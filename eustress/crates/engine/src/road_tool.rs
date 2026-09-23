//! Road-builder Studio plugin: lays a road out as a terrain spline layer.
//!
//! The first real consumer of the `studio_plugins` pipeline
//! (`PluginApi`/`TabRegistry`/`PluginActionEvent`), which existed as
//! complete, working Rust-side infrastructure but was entirely
//! disconnected: nothing ever registered a plugin, and no Slint code ever
//! rendered `TabRegistry`'s contents. This plugin registers for real; the
//! Slint side (`ui/slint/ribbon.slint` + `ui/slint_ui.rs`) renders it.
//!
//! Two distinct "plugin" concepts are both needed here, and must not be
//! confused:
//! - [`RoadToolEnginePlugin`], a Bevy `Plugin` (compile-time), wires this
//!   module's OWN systems into the app schedule.
//! - [`RoadToolPlugin`], a `StudioPlugin` (runtime trait object), lives in
//!   `PluginRegistry`, contributes tab/section/button UI via `PluginApi`.
//!
//! Node placement runs through real ECS systems and queued world commands
//! here, never touching `World` directly from inside [`RoadToolPlugin`]
//! itself, which stays a thin UI registration shim. That split is
//! deliberate: it's what makes this plugin a faithful proof of the
//! `PluginApi` surface a future Luau binding would need, rather than a
//! special-cased shortcut.
//!
//! ## What a road is
//! A road is a `TerrainSpline` in Road mode under `Workspace/Terrain/Layers`
//! with one `TerrainSplinePoint` child per placed node, each created through
//! the canonical `instance_create` path. So the Explorer lists the road under
//! the Terrain, Properties edits its width, shoulders, smoothing and
//! materials, the Move tool moves its nodes, and it saves like any instance.
//! The spline is a non-destructive layer: the bake carves its corridor over
//! the terrain's base as the nodes go down, and puts the base back where the
//! road was when it is removed or disabled, so this tool never writes the
//! raster. The drivable ribbon and its collider are laid along the same
//! baked stations, on the finished ground, by
//! `eustress_common::terrain::road_surface`, which does that for every Road
//! spline, however it was made. A new road takes an Order above every layer
//! already baked, so it carves last, over the ground the user clicked on.
//!
//! ## Undo
//! Each placed node is an instance creation (`Action::SpawnFolders`); the
//! first node of a road creates the road with it, so undoing that removes the
//! whole road. Remove Road goes through the Studio's own Delete, which trashes
//! the road's folder with its nodes and records the delete.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use eustress_common::classes::ClassName;
use eustress_common::terrain::layer_instances::{compose_world_pose, layers_dir, parse_layer_instance};
use eustress_common::terrain::layers::SplineMode;
use eustress_common::terrain::{TerrainBaked, TerrainConfig, TerrainData, TerrainRoot, TerrainSpline, TerrainSplinePoint};

use crate::entity_utils::entity_to_id_string;
use crate::modal_tool::{
    ActiveModalTool, ModalTool, ModalToolRegistry, ToolContext, ToolOptionControl, ToolStepResult, ViewportHit,
};
use crate::space::instance_create::{create_instance, InstanceOverrides};
use crate::space::instance_loader::InstanceFile;
use crate::studio_plugins::{PluginActionEvent, PluginApi, PluginCategory, PluginInfo, StudioPlugin, TabButtonSize};

/// Modal tool id of the node placer.
const PLACE_TOOL_ID: &str = "road_add_node";
/// Name a new road's spline asks for; `instance_create` makes it unique.
const ROAD_NAME: &str = "Road";
/// Undo labels: the first node of a road creates the road with it.
const START_ROAD_UNDO_LABEL: &str = "Start Road";
const ADD_NODE_UNDO_LABEL: &str = "Add Road Node";

// ============================================================================
// Resources
// ============================================================================

/// The folder of the road being laid out: the `TerrainSpline` the next
/// placed node extends. Kept as the folder rather than the entity because the
/// file watcher spawns a new spline some frames after the tool creates it,
/// and clicks can land in between.
#[derive(Resource, Default)]
struct ActiveRoad(Option<PathBuf>);

// ============================================================================
// Bevy Plugin: wires this module's systems into the schedule
// ============================================================================

pub struct RoadToolEnginePlugin;

impl Plugin for RoadToolEnginePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveRoad>()
            .add_systems(Startup, register_road_tools)
            // Own MessageReader<PluginActionEvent> cursor, independent of
            // the existing `handle_plugin_action_events`: Bevy Messages
            // support multiple independent readers. Runs after Drain for
            // the same reason that system does: Slint writes the event
            // during Drain, this frame.
            .add_systems(Update, (forget_road_on_space_switch, handle_road_tool_actions)
                .chain()
                .after(crate::ui::slint_ui::SlintSystems::Drain));
    }
}

/// The road being laid out belongs to the open Space. Its folder stays on
/// disk after a Space switch, so without this the next node would land in the
/// Space just left.
fn forget_road_on_space_switch(space_root: Option<Res<crate::space::SpaceRoot>>, mut active_road: ResMut<ActiveRoad>) {
    if space_root.is_some_and(|root| root.is_changed()) && active_road.0.is_some() {
        active_road.0 = None;
    }
}

/// Publish the node placer to [`ModalToolRegistry`] so
/// `ActivateModalToolEvent { tool_id: "road_add_node" }` actually resolves.
/// Without this the id lived ONLY inside [`RoadNodePlaceTool::id`]: the
/// ribbon button reached the tool by constructing it directly, but every
/// keybound / scripted / MCP activation fell through to
/// `activate_modal_tool_system`'s "Unknown modal tool id" warn path.
fn register_road_tools(mut registry: ResMut<ModalToolRegistry>) {
    registry.register(PLACE_TOOL_ID, || Box::new(RoadNodePlaceTool::default()));
}

// ============================================================================
// StudioPlugin: registers the tab/section/buttons (the actual UI surface)
// ============================================================================

#[derive(Default)]
pub struct RoadToolPlugin;

impl StudioPlugin for RoadToolPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "road-tool".to_string(),
            name: "Road Builder".to_string(),
            version: "0.2.0".to_string(),
            author: "Eustress".to_string(),
            description: "Spline road builder: lays roads out as terrain layers that carve the ground non-destructively."
                .to_string(),
            icon: None,
            category: PluginCategory::Building,
            permissions: Vec::new(),
        }
    }

    fn on_enable(&mut self, api: &mut PluginApi) {
        // Explicit tab id "plugins": the Slint side (ribbon.slint) matches
        // on this exact id for its 9th tab pill. `add_tab_section`'s doc
        // comment claims an empty tab_id auto-targets a "default Plugins
        // tab", but `sync_plugin_tabs` (mod.rs) does a plain id lookup with
        // no such special case; that auto-default isn't actually
        // implemented, so this registers its own tab explicitly instead of
        // relying on it.
        api.register_tab("plugins", "Plugins", None::<String>, 0, "road-tool");
        // Road Builder lives only in the Civil mode's Plugins tab (its terrain
        // road workflow is civil-engineering-specific).
        api.add_tab_section_scoped("plugins", "road", "Road Builder", vec!["civil".to_string()]);
        api.add_tab_button("plugins", "road", "road-add-node", "Add Node", Some("+"),
            "Click points on the terrain to lay out a road, or to extend the selected road", "road:add_node",
            TabButtonSize::Normal);
        api.add_tab_button("plugins", "road", "road-finish", "Finish Road", Some("="),
            "Stop laying out this road and select it; the next Add Node starts a new road", "road:finish_road",
            TabButtonSize::Normal);
        api.add_tab_button("plugins", "road", "road-remove", "Remove Road", Some("x"),
            "Delete the selected road, or the one being laid out; the terrain under it comes back", "road:remove_road",
            TabButtonSize::Normal);
    }
}

// ============================================================================
// Action dispatch: the real logic, driven by the SAME PluginActionEvent
// Slint buttons actually fire (confirmed: on_plugin_action -> SlintAction::
// PluginAction -> PluginActionEvent; StudioPlugin::on_menu_action is NOT on
// this path, it's driven by a separate, currently-unused event type).
// ============================================================================

type RoadSplines<'w, 's> = Query<'w, 's, (Entity, &'static InstanceFile, &'static TerrainSpline)>;
type RoadPoints<'w, 's> = Query<'w, 's, (Entity, &'static ChildOf), With<TerrainSplinePoint>>;

#[allow(clippy::too_many_arguments)]
fn handle_road_tool_actions(
    mut events: MessageReader<PluginActionEvent>,
    mut commands: Commands,
    terrain_query: Query<(), With<TerrainRoot>>,
    mut active_road: ResMut<ActiveRoad>,
    mut active_modal_tool: ResMut<ActiveModalTool>,
    splines: RoadSplines,
    points: RoadPoints,
    registry: Option<Res<crate::space::SpaceFileRegistry>>,
    // The selection the Studio's Delete reads, which Remove Road hands the
    // road to.
    selection: Option<Res<crate::selection_sync::SelectionSyncManager>>,
    mut pending_select: Option<ResMut<crate::ui::slint_ui::PendingInsertSelection>>,
    mut menu_events: MessageWriter<crate::ui::MenuActionEvent>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
) {
    // The road being laid out counts only inside the open Space (see
    // `place_road_node`): its folder outlives a Space switch on disk. Resolved
    // per use, since the fallback root reads the editor settings file.
    let in_open_space = |dir: &PathBuf| dir.starts_with(space_root_of(space_root.as_deref()));
    for event in events.read() {
        match event.action_id.as_str() {
            "road:add_node" => {
                // Terrain presence is checked HERE only so the user gets a
                // useful toast instead of a placer that silently swallows
                // every click. The tool re-resolves the terrain per click.
                if terrain_query.is_empty() {
                    notifications.warning("No active terrain. Generate terrain before adding a road.");
                    continue;
                }
                // A selected road is the one to extend, so a road from an
                // earlier session (or one finished a moment ago) can grow
                // again; otherwise the road being laid out, if it still
                // exists; otherwise the first click starts a new one.
                let extending = match selected_road(selection.as_deref(), &splines, &points) {
                    Some((_, folder)) => {
                        active_road.0 = Some(folder);
                        true
                    }
                    None => live_road(active_road.0.as_deref()).filter(|dir| in_open_space(dir)).is_some(),
                };

                // The road and each node's index are resolved per click
                // inside the tool (see `place_road_node`) rather than
                // snapshotted here. That's what keeps the tool zero-arg
                // constructible, so this button arms the EXACT same instance
                // the `ModalToolRegistry` factory builds: no second,
                // drift-prone construction path.
                active_modal_tool.activate(Box::new(RoadNodePlaceTool::default()), &mut commands);
                notifications.info(if extending {
                    "Click points on the terrain to extend the road. Right-click or Esc to stop."
                } else {
                    "Click points on the terrain to lay out a new road. Right-click or Esc to stop."
                });
            }

            "road:finish_road" => {
                if active_modal_tool.id() == Some(PLACE_TOOL_ID) {
                    active_modal_tool.cancel(&mut commands);
                }
                let Some(folder) = live_road(active_road.0.take().as_deref()).filter(|dir| in_open_space(dir)) else {
                    notifications.info("No road is being laid out");
                    continue;
                };
                // Selected, so Properties shows what there is to tune. A road
                // created a moment ago may not have spawned yet; the Insert
                // selection queue selects it the frame it does.
                match road_entity(&folder, registry.as_deref(), &splines) {
                    Some(entity) => {
                        if let Some(sm) = selection.as_deref() {
                            sm.0.write().select(entity_to_id_string(entity));
                        }
                    }
                    None => {
                        if let Some(pending) = pending_select.as_deref_mut() {
                            pending.waiting.push((folder, 0));
                        }
                    }
                }
                notifications.success(
                    "Road finished. Its width, shoulders, smoothing and materials are in Properties; the next Add Node starts a new road.",
                );
            }

            "road:remove_road" => {
                let target = match selected_road(selection.as_deref(), &splines, &points) {
                    Some((entity, folder)) => Some((Some(entity), folder)),
                    None => live_road(active_road.0.as_deref())
                        .filter(|dir| in_open_space(dir))
                        .map(|folder| (road_entity(&folder, registry.as_deref(), &splines), folder)),
                };
                let Some((entity, folder)) = target else {
                    notifications.info("No road to remove. Select a road, or lay one out with Add Node.");
                    continue;
                };
                let Some(entity) = entity else {
                    notifications.warning("The road is still loading. Try Remove Road again in a moment.");
                    continue;
                };
                let Some(sm) = selection.as_deref() else {
                    notifications.error("Remove Road needs the selection, which is not available");
                    continue;
                };
                if active_modal_tool.id() == Some(PLACE_TOOL_ID) {
                    active_modal_tool.cancel(&mut commands);
                }
                if active_road.0.as_deref() == Some(folder.as_path()) {
                    active_road.0 = None;
                }
                // The Studio's own Delete, on the road alone: it trashes the
                // road's folder with its nodes, purges them from the WorldDb
                // and records the delete for undo, exactly as the Delete key
                // would. The bake then drops the layer, which puts the
                // terrain under it back.
                sm.0.write().select(entity_to_id_string(entity));
                menu_events.write(crate::ui::MenuActionEvent::new(crate::keybindings::Action::Delete));
                notifications.success("Road removed and the terrain under it restored. Undo brings the road back.");
            }

            _ => {}
        }
    }
}

/// The road the selection names: a selected `TerrainSpline` in Road mode, or
/// the spline of a selected point. Returns its entity and folder.
fn selected_road(
    selection: Option<&crate::selection_sync::SelectionSyncManager>,
    splines: &RoadSplines<'_, '_>,
    points: &RoadPoints<'_, '_>,
) -> Option<(Entity, PathBuf)> {
    let selected: HashSet<String> = selection?.0.read().get_selected().into_iter().collect();
    if selected.is_empty() {
        return None;
    }
    let is_selected = |entity: Entity| selected.contains(&entity_to_id_string(entity));
    let road = |entity: Entity| -> Option<(Entity, PathBuf)> {
        let (entity, file, spline) = splines.get(entity).ok()?;
        (spline.mode == SplineMode::Road).then(|| (entity, folder_of(&file.toml_path)))
    };
    splines
        .iter()
        .filter(|(entity, _, _)| is_selected(*entity))
        .find_map(|(entity, _, _)| road(entity))
        .or_else(|| {
            points
                .iter()
                .filter(|(entity, _)| is_selected(*entity))
                .find_map(|(_, child_of)| road(child_of.parent()))
        })
}

/// The live entity of the road whose folder is `folder`, once it has spawned.
fn road_entity(
    folder: &Path,
    registry: Option<&crate::space::SpaceFileRegistry>,
    splines: &RoadSplines<'_, '_>,
) -> Option<Entity> {
    let toml_path = folder.join("_instance.toml");
    registry
        .and_then(|registry| registry.get_entity(&toml_path).or_else(|| registry.get_entity(folder)))
        .filter(|entity| splines.contains(*entity))
        .or_else(|| {
            splines
                .iter()
                .find(|(_, file, _)| file.toml_path == toml_path)
                .map(|(entity, _, _)| entity)
        })
}

/// The folder of a folder-form instance, from its `_instance.toml`.
fn folder_of(toml_path: &Path) -> PathBuf {
    toml_path.parent().map_or_else(|| toml_path.to_path_buf(), Path::to_path_buf)
}

/// The road being laid out, when it still exists: the tool remembers its
/// folder across a delete or an undone creation, and can extend neither.
fn live_road(active: Option<&Path>) -> Option<PathBuf> {
    active
        .filter(|folder| crate::terrain_layers::instance_text(&folder.join("_instance.toml")).is_some())
        .map(Path::to_path_buf)
}

/// The open Space's root folder.
fn space_root_of(resource: Option<&crate::space::SpaceRoot>) -> PathBuf {
    resource.map_or_else(crate::space::default_space_root, |root| root.0.clone())
}

// ============================================================================
// RoadNodePlaceTool: click-to-place modal tool
// ============================================================================

/// Click-to-place road nodes.
///
/// Carries NO terrain snapshot, no road and no index counter: every click
/// queues [`place_road_node`], which resolves all three from the World.
/// That is what makes the tool zero-arg constructible, which in turn is what
/// [`ModalToolRegistry`] factories require (`Fn() -> Box<dyn ModalTool>`).
#[derive(Default)]
struct RoadNodePlaceTool {
    /// Clicks accepted this session. Drives the step label only; the real
    /// node index comes from the road's points at placement time.
    placed_this_session: u32,
}

impl ModalTool for RoadNodePlaceTool {
    fn id(&self) -> &'static str { PLACE_TOOL_ID }
    fn name(&self) -> &'static str { "Road: Add Node" }

    fn step_label(&self) -> String {
        format!("Click to place a road node ({} placed; Esc or right-click to stop)", self.placed_this_session)
    }

    fn options(&self) -> Vec<ToolOptionControl> { Vec::new() }

    fn on_click(&mut self, hit: &ViewportHit, ctx: &mut ToolContext) -> ToolStepResult {
        let (origin, direction) = (hit.ray_origin, hit.ray_direction);
        ctx.commands.queue(move |world: &mut World| {
            place_road_node(world, origin, direction);
        });
        // Counted optimistically: the closure runs after this returns and
        // drops clicks that miss the terrain, so the label can read one high
        // after a stray click at the sky. The label is advisory; round-
        // tripping the real result back into the tool isn't worth a resource.
        self.placed_this_session += 1;
        ToolStepResult::Continue
    }

    fn commit(&mut self, _world: &mut World) {
        // Nodes are created as they are clicked (each click is its own
        // complete placement, not a staged preview), so there is nothing
        // left to commit. The tool ends via Cancel (Esc/right-click).
    }

    fn cancel(&mut self, _commands: &mut Commands) {
        // No preview entities are held (see `preview_entities` below):
        // already-placed nodes are real, persistent instances and must
        // survive a Cancel.
    }

    fn auto_exit_on_commit(&self) -> bool { false }

    fn preview_entities(&self) -> Vec<Entity> { Vec::new() }
}

/// Place ONE road node at the terrain point under the given viewport ray:
/// a new `TerrainSplinePoint` at the end of the road being laid out, or, when
/// there is none, a new road (a Road-mode `TerrainSpline` at the point, with
/// its first point on it). Both go through `instance_create`; the file
/// watcher spawns them and the layer bake carves the road once it has two
/// points. Each placement is one undo step.
///
/// Runs as a queued command closure rather than inline in
/// [`RoadNodePlaceTool::on_click`] because `ToolContext` hands a tool only
/// `Commands` + `Time`, no component or resource reads at all.
///
/// `ViewportHit::hit_point` is deliberately NOT used for placement: it's the
/// nearest physics hit of ANY collider (a part, a vehicle or the road's own
/// surface would catch the node), with a flat ground-plane fallback when the
/// ray hits nothing. This re-raycasts the terrain heightfield itself via
/// `terrain::height_query::raycast_terrain` instead.
fn place_road_node(world: &mut World, ray_origin: Vec3, ray_direction: Vec3) {
    let ray = Ray3d::new(ray_origin, Dir3::new(ray_direction).unwrap_or(Dir3::NEG_Y));

    // Scoped so the immutable terrain borrow is released before the writes
    // below take `&mut World`. Also reads the highest Order among the layers
    // already baked, which a new road is placed above.
    let (hit, top_order) = {
        let mut q = world.query_filtered::<(&TerrainConfig, &TerrainData, Option<&TerrainBaked>), With<TerrainRoot>>();
        let Some((config, data, baked)) = q.iter(world).next() else { return };
        let top = baked.and_then(|b| b.layers().iter().map(|l| l.order).max());
        // The ground the user sees, terrain layers included.
        let surface = eustress_common::terrain::surface_data(data, baked);
        (eustress_common::terrain::height_query::raycast_terrain(config, surface, ray, 5000.0, 2.0), top)
    };
    // Missed the terrain entirely: stay silent, the tool is still armed and
    // the user just clicks again.
    let Some(world_pos) = hit else { return };

    let space_root = space_root_of(world.get_resource::<crate::space::SpaceRoot>());
    // Re-checked because the road can be deleted, or its creation undone,
    // while the placer is still armed. Also filtered to the open Space: a
    // Space switch can land earlier in this frame than
    // `forget_road_on_space_switch`, and the old road's file still exists on
    // disk, so without this the node would go into the Space just left while
    // its undo entry names the new one.
    let active = world
        .get_resource::<ActiveRoad>()
        .and_then(|road| live_road(road.0.as_deref()))
        .filter(|dir| dir.starts_with(&space_root));

    let placed = match active {
        Some(spline_dir) => {
            // Points sit under their spline, which the user may have moved
            // or turned since it was created.
            let local = spline_pose(world, &spline_dir).compute_affine().inverse().transform_point3(world_pos);
            crate::terrain_layers::append_spline_point(&spline_dir, local).map(|point| (point, ADD_NODE_UNDO_LABEL))
        }
        None => start_road(&space_root, world_pos, top_order).map(|spline_dir| {
            if let Some(mut road) = world.get_resource_mut::<ActiveRoad>() {
                road.0 = Some(spline_dir.clone());
            }
            (spline_dir, START_ROAD_UNDO_LABEL)
        }),
    };

    match placed {
        Ok((folder, label)) => {
            if let Some(mut undo) = world.get_resource_mut::<crate::undo::UndoStack>() {
                undo.push_labeled(label, crate::undo::Action::spawn_folders(&space_root, &[folder]));
            }
        }
        Err(e) => {
            warn!("Road: placing a node failed: {e}");
            if let Some(mut notifications) = world.get_resource_mut::<crate::notifications::NotificationManager>() {
                notifications.error(format!("Could not place a road node: {e}"));
            }
        }
    }
}

/// Start a road at `at`: a `TerrainSpline` there in the Space's layer folder,
/// and its first point on it. The class template is a Road-mode spline (the
/// `class_templates_match_the_defaults` test holds it to
/// `TerrainSpline::default()`). It is written back with an Order one above
/// `top_order`, the highest baked layer, and left untouched on bare terrain.
/// Returns the spline's folder.
fn start_road(space_root: &Path, at: Vec3, top_order: Option<i32>) -> Result<PathBuf, String> {
    let overrides = InstanceOverrides { position: Some(at), ..Default::default() };
    let spline = create_instance(&layers_dir(space_root), ClassName::TerrainSpline.as_str(), Some(ROAD_NAME), overrides)
        .map_err(|e| e.to_string())?;
    // A road flattens its bed to its own profile, so it must apply after
    // every layer already under it: a layer applied later would reshape the
    // bed its ribbon lies on, which the ribbon follows only along its
    // centreline. Equal orders would leave that to the uuid.
    let order = top_order.map_or(0, |o| i64::from(o) + 1);
    if order != 0 {
        if let Err(e) = crate::particles::bridge::save_class_section(
            &spline.toml_path,
            &TerrainSpline { order, ..TerrainSpline::default() },
        ) {
            warn!("Road: ordering {} failed: {e}", spline.toml_path.display());
        }
    }
    // A failed first point leaves an empty road, which the next click
    // extends from its first point on.
    if let Err(e) = crate::terrain_layers::append_spline_point(&spline.folder_path, Vec3::ZERO) {
        warn!("Road: the first node of {} failed: {e}", spline.folder_path.display());
    }
    Ok(spline.folder_path)
}

/// World pose of the spline whose folder is `spline_dir`: its live entity's
/// `Transform` under its ancestors' once it has spawned, composed by the same
/// `compose_world_pose` the layer bake uses (`GlobalTransform` lags a frame),
/// else the pose its file gives, under a Workspace that adds nothing.
fn spline_pose(world: &World, spline_dir: &Path) -> Transform {
    let toml_path = spline_dir.join("_instance.toml");
    let entity = world
        .get_resource::<crate::space::SpaceFileRegistry>()
        .and_then(|registry| registry.get_entity(&toml_path).or_else(|| registry.get_entity(spline_dir)))
        .filter(|entity| world.get::<TerrainSpline>(*entity).is_some());
    if let Some(entity) = entity {
        return compose_world_pose(
            entity,
            |e| world.get::<Transform>(e).copied(),
            |e| world.get::<ChildOf>(e).map(ChildOf::parent),
        );
    }
    crate::terrain_layers::instance_text(&toml_path)
        .and_then(|text| parse_layer_instance(&text, ROAD_NAME).ok().flatten())
        .map(|(spline, _)| spline.transform)
        .unwrap_or_default()
}
