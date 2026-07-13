//! Road-builder Studio plugin — spline S-curve road-node tool.
//!
//! The first real consumer of the `studio_plugins` pipeline
//! (`PluginApi`/`TabRegistry`/`PluginActionEvent`), which existed as
//! complete, working Rust-side infrastructure but was entirely
//! disconnected — nothing ever registered a plugin, and no Slint code ever
//! rendered `TabRegistry`'s contents. This plugin registers for real; the
//! Slint side (`ui/slint/ribbon.slint` + `ui/slint_ui.rs`) renders it.
//!
//! Two distinct "plugin" concepts are both needed here, and must not be
//! confused:
//! - [`RoadToolEnginePlugin`] — a Bevy `Plugin` (compile-time), wires this
//!   module's OWN systems into the app schedule.
//! - [`RoadToolPlugin`] — a `StudioPlugin` (runtime trait object), lives in
//!   `PluginRegistry`, contributes tab/section/button UI via `PluginApi`.
//!
//! Node placement, terrain conform, and the ribbon mesh are handled
//! entirely through real ECS systems here — never touching `World`
//! directly from inside [`RoadToolPlugin`] itself, which stays a thin UI
//! registration shim. That split is deliberate: it's what makes this
//! plugin a faithful proof of the `PluginApi` surface a future Luau
//! binding would need, rather than a special-cased shortcut.
//!
//! Scope (v1, disclosed): road width/shoulder-falloff are fixed constants,
//! not yet Properties-panel-editable `NumberValue` children — a small,
//! low-risk fast-follow, not a blocker for a working, drivable road.

use bevy::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};

use eustress_common::classes::{Instance, ClassName, Attachment, BasePart, Part, PartType, Material, Model};
use eustress_common::attributes::{Attributes, Tags};
use eustress_common::terrain::{TerrainConfig, TerrainData, TerrainRoot};
use eustress_common::terrain::road::{build_road_path, conform_terrain_to_road, build_ribbon_mesh, RoadProfile};

use avian3d::prelude::{Collider, RigidBody};

use crate::studio_plugins::{StudioPlugin, PluginApi, PluginInfo, PluginCategory, TabButtonSize, PluginActionEvent};
use crate::modal_tool::{ModalTool, ToolContext, ToolStepResult, ToolOptionControl, ViewportHit, ActiveModalTool};
use crate::rendering::PartEntity;

fn entity_id_str(e: Entity) -> String {
    format!("{}v{}", e.index(), e.generation())
}

const TAG_ROAD_ROOT: &str = "road_root";
const TAG_ROAD_NODE: &str = "road_node";
const TAG_ROAD_SURFACE: &str = "road_surface";
const NODE_NAME_PREFIX: &str = "RoadNode_";

fn default_profile() -> RoadProfile {
    RoadProfile { half_width: 4.0, shoulder_falloff: 6.0 }
}

// ============================================================================
// Resources
// ============================================================================

/// Pristine terrain snapshot captured on the road's FIRST "Apply to
/// Terrain". Every later apply re-stamps from this, never from the
/// already-carved live `TerrainData` — see `terrain::road` module docs on
/// why (repeated apply would otherwise dig a trench).
#[derive(Resource, Default)]
struct RoadBaseline(Option<TerrainData>);

/// The Road `Model` entity currently being authored — this tool works on
/// one road at a time (no junctions/multi-road authoring in v1).
#[derive(Resource, Default)]
struct ActiveRoad(Option<Entity>);

// ============================================================================
// Bevy Plugin — wires this module's systems into the schedule
// ============================================================================

pub struct RoadToolEnginePlugin;

impl Plugin for RoadToolEnginePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RoadBaseline>()
            .init_resource::<ActiveRoad>()
            // Own MessageReader<PluginActionEvent> cursor, independent of
            // the existing `handle_plugin_action_events` — Bevy Messages
            // support multiple independent readers. Runs after Drain for
            // the same reason that system does: Slint writes the event
            // during Drain, this frame.
            .add_systems(Update, handle_road_tool_actions
                .after(crate::ui::slint_ui::SlintSystems::Drain));
    }
}

// ============================================================================
// StudioPlugin — registers the tab/section/buttons (the actual UI surface)
// ============================================================================

#[derive(Default)]
pub struct RoadToolPlugin;

impl StudioPlugin for RoadToolPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "road-tool".to_string(),
            name: "Road Builder".to_string(),
            version: "0.1.0".to_string(),
            author: "Eustress".to_string(),
            description: "Spline S-curve road-node builder that conforms to terrain.".to_string(),
            icon: None,
            category: PluginCategory::Building,
            permissions: Vec::new(),
        }
    }

    fn on_enable(&mut self, api: &mut PluginApi) {
        // Explicit tab id "plugins" — the Slint side (ribbon.slint) matches
        // on this exact id for its 9th tab pill. `add_tab_section`'s doc
        // comment claims an empty tab_id auto-targets a "default Plugins
        // tab", but `sync_plugin_tabs` (mod.rs) does a plain id lookup with
        // no such special case — that auto-default isn't actually
        // implemented, so this registers its own tab explicitly instead of
        // relying on it.
        api.register_tab("plugins", "Plugins", None::<String>, 0, "road-tool");
        api.add_tab_section("plugins", "road", "Road Builder");
        api.add_tab_button("plugins", "road", "road-add-node", "Add Node", Some("+"),
            "Click points on the terrain to lay out the road's S-curve", "road:add_node", TabButtonSize::Normal);
        api.add_tab_button("plugins", "road", "road-apply", "Apply to Terrain", Some("~"),
            "Conform the terrain to the current road path (cut + fill)", "road:apply_to_terrain", TabButtonSize::Normal);
        api.add_tab_button("plugins", "road", "road-remove", "Remove Road", Some("x"),
            "Delete the road and restore the terrain baseline", "road:remove_road", TabButtonSize::Normal);
    }
}

// ============================================================================
// Action dispatch — the real logic, driven by the SAME PluginActionEvent
// Slint buttons actually fire (confirmed: on_plugin_action -> SlintAction::
// PluginAction -> PluginActionEvent; StudioPlugin::on_menu_action is NOT on
// this path, it's driven by a separate, currently-unused event type).
// ============================================================================

#[allow(clippy::too_many_arguments)]
fn handle_road_tool_actions(
    mut events: MessageReader<PluginActionEvent>,
    mut commands: Commands,
    mut terrain_query: Query<(&TerrainConfig, &mut TerrainData), With<TerrainRoot>>,
    mut baseline: ResMut<RoadBaseline>,
    mut active_road: ResMut<ActiveRoad>,
    mut active_modal_tool: ResMut<ActiveModalTool>,
    node_query: Query<(Entity, &Instance, &Transform, &Tags, &ChildOf)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
) {
    for event in events.read() {
        match event.action_id.as_str() {
            "road:add_node" => {
                let Ok((config, data)) = terrain_query.single() else {
                    notifications.warning("No active terrain — generate terrain before adding a road");
                    continue;
                };

                // Reuse the existing road if one's already being authored;
                // otherwise start a fresh one.
                let road_entity = match active_road.0 {
                    Some(e) => e,
                    None => {
                        let instance = Instance {
                            name: "Road".to_string(),
                            class_name: ClassName::Model,
                            archivable: true,
                            ..Default::default()
                        };
                        let e = commands.spawn((
                            Transform::default(),
                            Visibility::default(),
                            instance,
                            Model::default(),
                            Name::new("Road"),
                            Attributes::new(),
                            {
                                let mut t = Tags::new();
                                t.add(TAG_ROAD_ROOT);
                                t
                            },
                        )).id();
                        active_road.0 = Some(e);
                        e
                    }
                };

                let next_index = node_query.iter()
                    .filter(|(_, _, _, tags, child_of)| tags.0.iter().any(|t| t == TAG_ROAD_NODE) && child_of.parent() == road_entity)
                    .count() as u32;

                active_modal_tool.activate(
                    Box::new(RoadNodePlaceTool::new(config.clone(), data.clone(), road_entity, next_index)),
                    &mut commands,
                );
                notifications.info("Click points on the terrain to place road nodes. Right-click or Esc to finish.");
            }

            "road:apply_to_terrain" => {
                let Some(road_entity) = active_road.0 else {
                    notifications.warning("No road to apply — click Add Node first");
                    continue;
                };
                let Ok((config, mut data)) = terrain_query.single_mut() else {
                    notifications.warning("No active terrain");
                    continue;
                };

                let mut control_points: Vec<(u32, Vec3)> = node_query.iter()
                    .filter(|(_, _, _, tags, child_of)| tags.0.iter().any(|t| t == TAG_ROAD_NODE) && child_of.parent() == road_entity)
                    .filter_map(|(_, inst, tf, _, _)| {
                        inst.name.strip_prefix(NODE_NAME_PREFIX)
                            .and_then(|n| n.parse::<u32>().ok())
                            .map(|idx| (idx, tf.translation))
                    })
                    .collect();
                control_points.sort_by_key(|(idx, _)| *idx);
                let points: Vec<Vec3> = control_points.into_iter().map(|(_, p)| p).collect();

                if points.len() < 2 {
                    notifications.warning("Place at least 2 road nodes before applying");
                    continue;
                }

                // Capture the pristine baseline on FIRST apply only — every
                // later apply re-stamps from this, never from `data` after
                // it's been carved (see terrain::road module docs).
                if baseline.0.is_none() {
                    baseline.0 = Some(data.clone());
                }
                let baseline_data = baseline.0.as_ref().unwrap();

                let Some(path) = build_road_path(config, baseline_data, &points, 2.0, 15.0) else {
                    notifications.error("Could not build road path from the placed nodes");
                    continue;
                };
                let profile = default_profile();
                let cell_size = config.chunk_size / config.chunk_resolution.max(1) as f32;
                let result = conform_terrain_to_road(config, baseline_data, &mut data, &path, profile, cell_size);

                spawn_or_update_ribbon(&mut commands, &mut meshes, &mut materials, road_entity, &path, profile, &node_query);

                notifications.success(format!("Road applied — {} terrain cells conformed", result.cells_written));
            }

            "road:remove_road" => {
                let Some(road_entity) = active_road.0.take() else {
                    notifications.info("No road to remove");
                    continue;
                };
                commands.entity(road_entity).despawn();

                // Restore the pristine baseline, if terrain was ever carved.
                if let Some(baseline_data) = baseline.0.take() {
                    if let Ok((_, mut data)) = terrain_query.single_mut() {
                        *data = baseline_data;
                    }
                }
                notifications.success("Road removed, terrain restored");
            }

            _ => {}
        }
    }
}

/// Build (or rebuild) the ribbon `MeshPart` entity + its per-segment
/// cuboid colliders. Cuboid segments, not a single trimesh collider —
/// `Collider::trimesh_from_mesh` is flagged unverified everywhere else it
/// appears in this codebase (terrain's own collider is commented out
/// pending that verification); per-segment cuboids reuse the SAME
/// `Collider::cuboid` primitive the CSG boolean-op custom-mesh Parts
/// already rely on (`csg.rs`), just chained along the curve instead of one
/// box — a proven primitive, not a new unverified one, and terrain
/// colliders being off project-wide makes this the ONLY way the road is
/// ever drivable.
fn spawn_or_update_ribbon(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    road_entity: Entity,
    path: &eustress_common::terrain::road::RoadPath,
    profile: RoadProfile,
    node_query: &Query<(Entity, &Instance, &Transform, &Tags, &ChildOf)>,
) {
    // Despawn any previous ribbon surface under this road before rebuilding
    // — re-applying after moving a node must replace, not accumulate.
    for (entity, _, _, tags, child_of) in node_query.iter() {
        if child_of.parent() == road_entity && tags.0.iter().any(|t| t == TAG_ROAD_SURFACE) {
            commands.entity(entity).despawn();
        }
    }

    let ribbon = build_ribbon_mesh(path, profile, 0.05);
    if ribbon.positions.len() < 4 {
        return;
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, ribbon.positions.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, ribbon.normals.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, ribbon.uvs.clone());
    mesh.insert_indices(Indices::U32(ribbon.indices.clone()));
    let mesh_handle = meshes.add(mesh);

    let material_handle = materials.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.16, 0.18),
        perceptual_roughness: 0.85,
        ..default()
    });

    let instance = Instance {
        name: "RoadSurface".to_string(),
        class_name: ClassName::Part,
        archivable: true,
        ..Default::default()
    };
    let mut bp = BasePart::default();
    bp.size = Vec3::ONE; // geometry lives in the mesh itself, not a primitive size
    bp.cframe = Transform::IDENTITY;
    bp.can_collide = true;
    bp.anchored = true;
    let part = Part { shape: PartType::Block }; // metadata only — Mesh3d below is the real geometry

    let surface_entity = commands.spawn((
        Mesh3d(mesh_handle),
        MeshMaterial3d(material_handle),
        Transform::IDENTITY,
        instance,
        bp,
        part,
        Name::new("RoadSurface"),
        PartEntity { part_id: String::new() },
        Attributes::new(),
        {
            let mut t = Tags::new();
            t.add(TAG_ROAD_SURFACE);
            t
        },
        ChildOf(road_entity),
    )).id();
    let part_id = entity_id_str(surface_entity);
    commands.entity(surface_entity).insert(PartEntity { part_id });

    // Per-segment oriented cuboid colliders, as separate child entities —
    // a curve can't be one axis-aligned box.
    for pair in path.stations.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let mid = (a.pos + b.pos) * 0.5;
        let seg_len = (b.pos - a.pos).length().max(0.01);
        let forward = (b.pos - a.pos).normalize_or_zero();
        let rotation = Transform::IDENTITY.looking_to(forward, Vec3::Y).rotation;
        commands.spawn((
            Transform::from_translation(mid).with_rotation(rotation),
            Collider::cuboid(profile.half_width, 0.15, seg_len * 0.5 + 0.05),
            RigidBody::Static,
            Name::new("RoadColliderSegment"),
            ChildOf(surface_entity),
        ));
    }
}

// ============================================================================
// RoadNodePlaceTool — click-to-place modal tool
// ============================================================================

/// Click-to-place road control nodes. Caches a snapshot of the active
/// terrain's `(TerrainConfig, TerrainData)` at construction time (taken by
/// the calling system, which has query access) because `ToolContext` only
/// exposes `Commands` + `Time` to `on_click` — not arbitrary component
/// reads. `ViewportHit::hit_point` is NOT used for placement: it's a
/// physics-raycast result that falls back to a flat ground plane when
/// nothing has a collider, which is exactly terrain's situation (terrain
/// colliders are disabled project-wide) — so it would place every node at
/// Y=0 on a mountain. This tool re-raycasts the REAL terrain surface from
/// `hit.ray_origin`/`ray_direction` via `terrain::road_query::raycast_terrain`
/// instead.
struct RoadNodePlaceTool {
    config: TerrainConfig,
    data: TerrainData,
    road_entity: Entity,
    next_index: u32,
    placed_this_session: u32,
}

impl RoadNodePlaceTool {
    fn new(config: TerrainConfig, data: TerrainData, road_entity: Entity, next_index: u32) -> Self {
        Self { config, data, road_entity, next_index, placed_this_session: 0 }
    }
}

impl ModalTool for RoadNodePlaceTool {
    fn id(&self) -> &'static str { "road_add_node" }
    fn name(&self) -> &'static str { "Road: Add Node" }

    fn step_label(&self) -> String {
        format!("Click to place road node {} (Esc/right-click to finish)", self.next_index + 1)
    }

    fn options(&self) -> Vec<ToolOptionControl> { Vec::new() }

    fn on_click(&mut self, hit: &ViewportHit, ctx: &mut ToolContext) -> ToolStepResult {
        let ray = Ray3d::new(hit.ray_origin, Dir3::new(hit.ray_direction).unwrap_or(Dir3::NEG_Y));
        let Some(world_pos) = eustress_common::terrain::height_query::raycast_terrain(&self.config, &self.data, ray, 5000.0, 2.0) else {
            return ToolStepResult::Continue; // Missed the terrain entirely — stay active, let the user try again.
        };

        let instance = Instance {
            name: format!("{}{}", NODE_NAME_PREFIX, self.next_index),
            class_name: ClassName::Attachment,
            archivable: true,
            ..Default::default()
        };
        ctx.commands.spawn((
            Transform::from_translation(world_pos),
            GlobalTransform::default(),
            Visibility::default(),
            instance,
            Attachment::default(),
            Name::new(format!("{}{}", NODE_NAME_PREFIX, self.next_index)),
            Attributes::new(),
            {
                let mut t = Tags::new();
                t.add(TAG_ROAD_NODE);
                t
            },
            ChildOf(self.road_entity),
        ));

        self.next_index += 1;
        self.placed_this_session += 1;
        ToolStepResult::Continue
    }

    fn commit(&mut self, _world: &mut World) {
        // Nodes are already spawned live in `on_click` (each click is its
        // own complete placement, not a staged preview) — nothing left to
        // commit. The tool ends via Cancel (Esc/right-click), not Commit.
    }

    fn cancel(&mut self, _commands: &mut Commands) {
        // No preview entities are held (see `preview_entities` below) —
        // already-placed nodes are real, persistent entities and must
        // survive a Cancel.
    }

    fn auto_exit_on_commit(&self) -> bool { false }

    fn preview_entities(&self) -> Vec<Entity> { Vec::new() }
}
