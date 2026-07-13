//! # CadPlugin — parametric feature-tree parts in the Studio
//!
//! Phase A vertical slice (CAD_PLATFORM_PLAN.md):
//! author a feature tree → tessellate → see solid in viewport →
//! change a variable → regenerate.
//!
//! ## Components
//!
//! [`CadPart`] holds the feature-tree TOML (source of truth). On
//! [`Changed<CadPart>`] the regenerate system parses, evaluates via
//! `eustress-cad`, and rebinds `Mesh3d` + collider + `BasePart.size`.
//!
//! ## Insert
//!
//! Ribbon / menu actions `insert:cad_plate` / `insert:cad_box` /
//! `insert:cad_cylinder` fire [`CadInsertTemplateEvent`]. Spawns a
//! Part-class entity with a default tree at the camera-forward point.
//!
//! ## Variables
//!
//! [`CadSetVariableEvent`] patches a variable in the tree TOML and
//! marks the part dirty (full re-eval).

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::asset::RenderAssetUsages;
use avian3d::prelude::{Collider, RigidBody};

use eustress_cad::{evaluate_tree, parse_tree, tree_to_toml, EvalMesh};
use eustress_common::attributes::{Attributes, Tags};
use eustress_common::classes::{BasePart, ClassName, Instance, Material, Part, PartType};

use crate::notifications::NotificationManager;
use crate::rendering::PartEntity;
use crate::selection_sync::SelectionSyncManager;

// ============================================================================
// Component
// ============================================================================

/// Parametric CAD body driven by an `eustress-cad` feature tree.
/// Mutating [`tree_toml`] triggers regenerate. Status lives on
/// [`CadPartStatus`] so regen feedback does not re-enter the loop.
#[derive(Component, Debug, Clone)]
pub struct CadPart {
    /// Feature-tree TOML (source of truth). Edit this → regenerate.
    pub tree_toml: String,
}

impl CadPart {
    pub fn new(tree_toml: impl Into<String>) -> Self {
        Self {
            tree_toml: tree_toml.into(),
        }
    }
}

/// Last evaluation outcome — updated by regenerate, never triggers it.
#[derive(Component, Debug, Clone, Default)]
pub struct CadPartStatus {
    pub ok: bool,
    pub message: String,
}

// ============================================================================
// Events
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CadTemplate {
    /// Thin rectangular plate (100×60×10 mm).
    Plate,
    /// Cube 50 mm on each side.
    Box,
    /// Vertical cylinder Ø40 × H60 mm.
    Cylinder,
    /// Plate with a centered through-hole (feature tree: extrude + hole).
    PlateWithHole,
    /// L-bracket: base plate + vertical flange via two extrudes + union.
    LBracket,
    /// Demo sketch with constraints (horizontal/vertical/perpendicular)
    /// so Solve Sketch has something to do out of the box.
    ConstrainedFrame,
    /// Thin shell demo (extrude box + shell feature).
    ShelledBox,
}

#[derive(Event, Message, Debug, Clone)]
pub struct CadInsertTemplateEvent {
    pub template: CadTemplate,
    /// World position; if None, plugin uses camera-forward heuristic.
    pub position: Option<Vec3>,
}

#[derive(Event, Message, Debug, Clone)]
pub struct CadSetVariableEvent {
    pub entity: Entity,
    pub name: String,
    pub value: String,
}

/// Export selected CadPart (or any entity with Mesh3d we can re-eval) to `.glb`.
#[derive(Event, Message, Debug, Clone)]
pub struct CadExportGlbEvent {
    pub entity: Entity,
    /// Destination path. If None, writes next to features.toml or
    /// `./exports/{name}.glb`.
    pub path: Option<std::path::PathBuf>,
}

/// Mutate the feature tree of a CadPart (suppress / reorder / delete).
#[derive(Event, Message, Debug, Clone)]
pub struct CadTreeOpEvent {
    pub entity: Entity,
    pub op: CadTreeOp,
}

/// Run the constraint solver on every sketch in the CadPart tree and
/// write solved entity coordinates back into features.toml.
#[derive(Event, Message, Debug, Clone)]
pub struct CadSolveSketchEvent {
    pub entity: Entity,
}

/// Add a geometric constraint to the first sketch of a CadPart.
#[derive(Event, Message, Debug, Clone)]
pub struct CadAddConstraintEvent {
    pub entity: Entity,
    pub kind: eustress_cad::ConstraintKind,
    /// First entity index (default 0).
    pub e1: usize,
    /// Second entity index when binary.
    pub e2: Option<usize>,
}

/// Force the sketch canvas open/closed from ribbon / Esc.
#[derive(Event, Message, Debug, Clone)]
pub struct CadSketchCanvasSetVisibleEvent {
    pub visible: bool,
}

/// UI projection of the selected CadPart's first sketch (for Slint).
#[derive(Resource, Debug, Clone, Default)]
pub struct CadSketchUiState {
    pub visible: bool,
    pub force_open: bool,
    /// Part the user explicitly closed the panel for. Auto-open stays
    /// suppressed while this part remains selected — without it, the
    /// close button is overwritten by `update_sketch_ui_state` in the
    /// same frame and the panel can never be dismissed.
    pub dismissed_for: Option<Entity>,
    pub part_entity: Option<Entity>,
    pub part_name: String,
    pub sketch_name: String,
    pub solve_status: String,
    pub entities: Vec<(usize, String, String)>, // index, kind, summary
    pub constraints: Vec<(usize, String, String)>, // index, kind, detail
    /// Entity indices picked in the panel for the next constraint —
    /// unary (H/V) uses `selected_a`; binary (⊥/⊙) needs both.
    pub selected_a: Option<usize>,
    pub selected_b: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum CadTreeOp {
    /// Suppress entry at index.
    Suppress { index: usize },
    /// Un-suppress entry at index.
    Unsuppress { index: usize },
    /// Move entry from → to.
    Reorder { from: usize, to: usize },
    /// Delete entry at index.
    Delete { index: usize },
}

// ============================================================================
// Plugin
// ============================================================================

pub struct CadPlugin;

impl Plugin for CadPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CadSketchUiState>()
            .add_message::<CadInsertTemplateEvent>()
            .add_message::<CadSetVariableEvent>()
            .add_message::<CadExportGlbEvent>()
            .add_message::<CadTreeOpEvent>()
            .add_message::<CadSolveSketchEvent>()
            .add_message::<CadAddConstraintEvent>()
            .add_message::<CadSketchCanvasSetVisibleEvent>()
            .add_systems(
                Update,
                (
                    // Disk polling on a timer, not per-frame — these
                    // touch the filesystem per candidate entity, and
                    // the fs is only mutated at human/agent cadence.
                    attach_cad_from_features_toml.run_if(bevy::time::common_conditions::on_timer(
                        std::time::Duration::from_millis(500),
                    )),
                    sync_cad_features_from_disk.run_if(bevy::time::common_conditions::on_timer(
                        std::time::Duration::from_millis(500),
                    )),
                    handle_cad_insert,
                    handle_cad_set_variable,
                    handle_cad_tree_ops,
                    handle_cad_solve_sketch,
                    handle_cad_add_constraint,
                    open_sketch_canvas_on_cadpart_double_click,
                    handle_sketch_canvas_visibility,
                    close_sketch_canvas_on_escape,
                    update_sketch_ui_state,
                    regenerate_cad_parts,
                    handle_cad_export_glb,
                )
                    .chain(),
            );
    }
}

/// Feature-tree rows for the Properties panel.
#[derive(Debug, Clone)]
pub struct FeatureTreeRow {
    pub index: usize,
    pub name: String,
    pub kind: String,
    pub suppressed: bool,
    pub ok: bool,
    pub message: String,
}

/// Parse tree + status into UI rows.
pub fn list_feature_rows(tree_toml: &str, status: &CadPartStatus) -> Vec<FeatureTreeRow> {
    let Ok(tree) = parse_tree(tree_toml) else {
        return vec![FeatureTreeRow {
            index: 0,
            name: "(parse error)".into(),
            kind: "error".into(),
            suppressed: false,
            ok: false,
            message: status.message.clone(),
        }];
    };
    // Status messages are parallel to non-suppressed walk — use entry names.
    let status_by_name: std::collections::HashMap<String, (bool, String)> = {
        // Re-eval status is stored as a flat message; rows use tree only for structure.
        let _ = status;
        std::collections::HashMap::new()
    };
    tree.entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let (ok, message) = status_by_name
                .get(e.name())
                .cloned()
                .unwrap_or((!e.is_suppressed(), String::new()));
            FeatureTreeRow {
                index: i,
                name: e.name().to_string(),
                kind: e.kind_label().to_string(),
                suppressed: e.is_suppressed(),
                ok,
                message,
            }
        })
        .collect()
}

/// List `[variables]` from a feature-tree TOML (name → raw expression).
pub fn list_variables(tree_toml: &str) -> Vec<(String, String)> {
    match parse_tree(tree_toml) {
        Ok(tree) => {
            let mut v: Vec<_> = tree.variables.into_iter().collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v
        }
        Err(_) => Vec::new(),
    }
}

/// Which feature-tree variable (if any) drives each Size axis, by
/// naming convention over the built-in templates (`eustress_cad::
/// templates`). CadParts are still `Part`-class Instances — Size is
/// the SAME `BasePart.size` XYZ every Part shows, not a parallel
/// "Var.height" concept; this table is what lets the Properties
/// panel's Size write-back reach the right variable(s).
///
/// A `None` axis means the template doesn't parametrically author
/// that dimension (e.g. `ConstrainedFrame`'s X/Y come out of the
/// solver, not a typed length) — Size still SHOWS the computed
/// extent there via `BasePart.size`, editing it is just a no-op.
///
/// `radius`-driven axes (Cylinder) map size ↔ diameter: the Size
/// field is a full extent like every other Part, so `radius = size /
/// 2` on write and `size = radius * 2` is implied on the read side
/// (BasePart.size already reflects the tessellated diameter, no
/// special-casing needed there).
pub fn size_axis_variables(tree_toml: &str) -> [Option<String>; 3] {
    let vars: std::collections::HashSet<String> =
        list_variables(tree_toml).into_iter().map(|(n, _)| n).collect();
    let has = |n: &str| vars.contains(n);
    if has("length") && has("width") && has("height") {
        [Some("length".into()), Some("width".into()), Some("height".into())]
    } else if has("radius") && has("height") {
        [Some("radius".into()), Some("radius".into()), Some("height".into())]
    } else if has("size") {
        [Some("size".into()), Some("size".into()), Some("size".into())]
    } else if has("height") {
        [None, None, Some("height".into())]
    } else if has("thickness") {
        [None, None, Some("thickness".into())]
    } else if has("depth") {
        [None, None, Some("depth".into())]
    } else {
        [None, None, None]
    }
}

/// `list_variables`, minus whatever `size_axis_variables` already
/// exposes through Size — feature-specific knobs only (hole
/// diameter, wall thickness, …), so Properties doesn't show the same
/// number twice under two different names.
pub fn list_secondary_variables(tree_toml: &str) -> Vec<(String, String)> {
    let axis_vars: std::collections::HashSet<String> =
        size_axis_variables(tree_toml).into_iter().flatten().collect();
    list_variables(tree_toml)
        .into_iter()
        .filter(|(name, _)| !axis_vars.contains(name))
        .collect()
}

// ============================================================================
// Disk attach — folder with features.toml becomes a CadPart
// ============================================================================

/// When a TOML-backed instance has a sibling `features.toml`, attach
/// [`CadPart`] so the parametric regen path owns the mesh. Runs once
/// per entity (skips if CadPart already present).
fn attach_cad_from_features_toml(
    mut commands: Commands,
    query: Query<
        (Entity, &crate::space::instance_loader::InstanceFile),
        (
            Without<CadPart>,
            With<crate::rendering::PartEntity>,
        ),
    >,
) {
    for (entity, inst) in query.iter() {
        let Some(parent) = inst.toml_path.parent() else {
            continue;
        };
        let features_path = parent.join("features.toml");
        if !features_path.is_file() {
            continue;
        }
        match std::fs::read_to_string(&features_path) {
            Ok(toml) => {
                info!(
                    "📐 Attaching CadPart from {:?}",
                    features_path
                );
                commands.entity(entity).insert(CadPart { tree_toml: toml });
            }
            Err(e) => {
                warn!("📐 Failed to read {:?}: {e}", features_path);
            }
        }
    }
}

/// Write `features.toml` next to the instance. A silently-failed write
/// here is worse than an error: the next `sync_cad_features_from_disk`
/// pass treats disk as authoritative and reverts the in-memory tree —
/// the user's edit "undoes itself" with no explanation.
fn write_features_toml(
    inst_file: Option<&crate::space::instance_loader::InstanceFile>,
    toml: &str,
) -> Result<(), String> {
    let Some(parent) = inst_file.and_then(|i| i.toml_path.parent()) else {
        // No disk backing (pure in-memory part) — nothing to persist.
        return Ok(());
    };
    let path = parent.join("features.toml");
    std::fs::write(&path, toml).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Hot-reload: if features.toml on disk differs from CadPart.tree_toml,
/// adopt the disk version (MCP / external editors). Cheap while few
/// CadParts exist.
fn sync_cad_features_from_disk(
    mut query: Query<(
        &mut CadPart,
        &crate::space::instance_loader::InstanceFile,
    )>,
) {
    for (mut cad, inst) in query.iter_mut() {
        let Some(parent) = inst.toml_path.parent() else {
            continue;
        };
        let features_path = parent.join("features.toml");
        let Ok(src) = std::fs::read_to_string(&features_path) else {
            continue;
        };
        if src != cad.tree_toml {
            cad.tree_toml = src;
            info!("📐 CadPart hot-reloaded from {:?}", features_path);
        }
    }
}

// ============================================================================
// Insert
// ============================================================================

fn handle_cad_insert(
    mut events: MessageReader<CadInsertTemplateEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    selection: Option<Res<SelectionSyncManager>>,
    mut notifications: Option<ResMut<NotificationManager>>,
    mut undo: Option<ResMut<crate::undo::UndoStack>>,
    mut registry: Option<ResMut<crate::space::SpaceFileRegistry>>,
) {
    for event in events.read() {
        let pos = event.position.unwrap_or_else(|| camera_forward_spawn(&cameras));
        let toml = template_toml(event.template);
        let label = template_label(event.template);

        match spawn_cad_entity(
            &mut commands,
            &mut meshes,
            &mut materials,
            toml,
            pos,
            label,
        ) {
            Ok((entity, folder)) => {
                if let Some(folder) = folder {
                    // Register the `_instance.toml` path — NOT the bare
                    // folder — so the file watcher doesn't spawn a
                    // duplicate of the entity we just created. The
                    // watcher's `handle_file_created` guard checks
                    // `registry.is_loaded(&event.path)` against the
                    // exact FILE path it saw change (the TOML), not the
                    // parent directory; registering only the folder is a
                    // no-op against that guard, so a second entity got
                    // spawned for every CAD insert once the watcher
                    // noticed the new `_instance.toml` on disk. Record
                    // undo — Ctrl+Z trashes the folder and despawns the
                    // part, redo restores both (same contract as the
                    // Smart Build Tools).
                    if let Some(ref mut reg) = registry {
                        let instance_toml = folder.join("_instance.toml");
                        let modified = std::fs::metadata(&folder)
                            .and_then(|m| m.modified())
                            .unwrap_or_else(|_| std::time::SystemTime::now());
                        let name = folder
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| label.to_string());
                        let metadata = crate::space::file_loader::FileMetadata {
                            path: instance_toml.clone(),
                            file_type: crate::space::file_loader::FileType::Toml,
                            service: "Workspace".to_string(),
                            name,
                            size: 0,
                            modified,
                            children: Vec::new(),
                        };
                        reg.register(instance_toml, entity, metadata);
                    }
                    if let Some(ref mut u) = undo {
                        let trash = crate::space::default_space_root()
                            .join(".eustress")
                            .join("trash")
                            .join(chrono::Utc::now().format("%Y%m%d_%H%M%S_%f").to_string())
                            .join(folder.file_name().unwrap_or_default());
                        u.push_labeled(
                            format!("Insert {label}"),
                            crate::undo::Action::SpawnFolders {
                                folders: vec![(folder, trash)],
                            },
                        );
                    }
                }
                if let Some(ref sel) = selection {
                    sel.0
                        .write()
                        .set_selected(vec![format!("{}v{}", entity.index(), entity.generation())]);
                }
                if let Some(ref mut n) = notifications {
                    n.success(format!(
                        "CadPart: inserted {label} — edit variables to regenerate"
                    ));
                }
                info!("📐 CadPart inserted ({label}) at {pos:?} → {:?}", entity);
            }
            Err(e) => {
                if let Some(ref mut n) = notifications {
                    n.warning(format!("CadPart insert failed: {e}"));
                }
                warn!("📐 CadPart insert failed: {e}");
            }
        }
    }
}

fn camera_forward_spawn(cameras: &Query<(&Camera, &GlobalTransform)>) -> Vec3 {
    if let Some((_, cam)) = cameras.iter().find(|(c, _)| c.order == 0) {
        let p = cam.translation() + cam.forward() * 8.0;
        Vec3::new(p.x, p.y.max(0.5), p.z)
    } else {
        Vec3::new(0.0, 0.5, 0.0)
    }
}

fn spawn_cad_entity(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    tree_toml: String,
    position: Vec3,
    name: &str,
) -> Result<(Entity, Option<std::path::PathBuf>), String> {
    let tree = parse_tree(&tree_toml).map_err(|e| format!("parse: {e}"))?;
    let out = evaluate_tree(&tree).map_err(|e| format!("eval: {e}"))?;
    let eval_mesh = out
        .mesh
        .filter(|m| !m.indices.is_empty())
        .ok_or_else(|| "evaluation produced no mesh".to_string())?;

    let (min, max) = mesh_bounds(&eval_mesh);
    let center_local = (min + max) * 0.5;
    let size = (max - min).max(Vec3::splat(0.01));
    let mut bevy_mesh = eval_mesh_to_bevy(&eval_mesh);
    bevy_mesh = translate_mesh(bevy_mesh, -center_local);

    let mesh_handle = meshes.add(bevy_mesh);
    let material_handle = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.62, 0.72),
        perceptual_roughness: 0.45,
        metallic: 0.15,
        ..default()
    });

    let status = format_status(&out.entry_status, true);
    let mut bp = BasePart::default();
    bp.size = size;
    bp.cframe = Transform::from_translation(position);
    bp.color = Color::srgb(0.55, 0.62, 0.72);
    bp.material = Material::Plastic;
    bp.can_collide = true;
    bp.anchored = true;

    // Every create surface stamps a real identity — a blank uuid is
    // permanent once persist_cad_folder writes it to disk (the loader
    // round-trips metadata.uuid verbatim).
    let uuid = eustress_common::instance_create::fresh_uuid_for_create();

    // Auto-save folder on insert so CadParts are git-diffable immediately.
    let (instance_file, display_name) = persist_cad_folder(name, &tree_toml, position, size, &uuid)
        .unwrap_or((None, name.to_string()));

    let half = size * 0.5;
    let mut entity_cmds = commands.spawn((
        Mesh3d(mesh_handle),
        MeshMaterial3d(material_handle),
        Transform::from_translation(position),
        Instance {
            name: display_name.clone(),
            class_name: ClassName::Part,
            archivable: true,
            id: 0,
            ai: false,
            uuid,
        },
        bp,
        Part {
            shape: PartType::Block,
        },
        CadPart { tree_toml },
        CadPartStatus {
            ok: true,
            message: status,
        },
        Collider::cuboid(half.x.max(0.001), half.y.max(0.001), half.z.max(0.001)),
        RigidBody::Static,
        Name::new(display_name),
        PartEntity {
            part_id: String::new(),
        },
        Attributes::new(),
        Tags::new(),
    ));
    let folder = instance_file
        .as_ref()
        .and_then(|f| f.toml_path.parent().map(|p| p.to_path_buf()));
    if let Some(inst) = instance_file {
        entity_cmds.insert(inst);
    }
    let entity = entity_cmds.id();

    let part_id = format!("{}v{}", entity.index(), entity.generation());
    commands.entity(entity).insert(PartEntity { part_id });
    Ok((entity, folder))
}

/// Write `Workspace/{Name}/_instance.toml` + `features.toml` and return
/// the InstanceFile component. Unique-suffixes the folder name if needed.
fn persist_cad_folder(
    base_name: &str,
    tree_toml: &str,
    position: Vec3,
    size: Vec3,
    uuid: &str,
) -> Result<(Option<crate::space::instance_loader::InstanceFile>, String), String> {
    let space_root = crate::space::default_space_root();
    let workspace = space_root.join("Workspace");
    std::fs::create_dir_all(&workspace).map_err(|e| format!("mkdir Workspace: {e}"))?;

    let folder_name =
        crate::space::instance_loader::unique_entity_name(&workspace, base_name);
    let instance_dir = workspace.join(&folder_name);
    std::fs::create_dir_all(&instance_dir).map_err(|e| format!("mkdir instance: {e}"))?;

    let toml_path = instance_dir.join("_instance.toml");
    let features_path = instance_dir.join("features.toml");

    let instance_toml = format!(
        r#"[metadata]
class_name = "Part"
archivable = true
name = "{folder_name}"
uuid = "{uuid}"

[transform]
position = [{}, {}, {}]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [{}, {}, {}]

[properties]
color = [140, 158, 184]
transparency = 0.0
anchored = true
can_collide = true
cast_shadow = true
reflectance = 0.0
material = "Plastic"
locked = false

[asset]
mesh = "parts/block.glb"
scene = "Scene0"
"#,
        position.x,
        position.y,
        position.z,
        size.x,
        size.y,
        size.z,
    );
    std::fs::write(&toml_path, instance_toml).map_err(|e| format!("write instance: {e}"))?;
    std::fs::write(&features_path, tree_toml).map_err(|e| format!("write features: {e}"))?;

    info!(
        "📐 Auto-saved CadPart folder → {:?}",
        instance_dir
    );

    Ok((
        Some(crate::space::instance_loader::InstanceFile {
            toml_path,
            mesh_path: std::path::PathBuf::from("parts/block.glb"),
            name: folder_name.clone(),
        }),
        folder_name,
    ))
}

/// Double-clicking a CadPart in the viewport opens its Sketch Canvas —
/// matches the Fusion/SolidWorks convention of double-click-to-edit a
/// feature/body. `update_sketch_ui_state` already resolves `part_entity`
/// from whatever CadPart is currently selected, and the click that
/// produces this double-click message has already updated selection to
/// the hit entity by the time this system runs — so firing the same
/// `CadSketchCanvasSetVisibleEvent` the ribbon's toggle uses is enough,
/// no separate entity-targeting plumbing needed.
fn open_sketch_canvas_on_cadpart_double_click(
    mut clicks: MessageReader<crate::part_selection::DoubleClickedPart>,
    cad_parts: Query<&CadPart>,
    mut visible: MessageWriter<CadSketchCanvasSetVisibleEvent>,
) {
    for click in clicks.read() {
        if cad_parts.get(click.entity).is_ok() {
            visible.write(CadSketchCanvasSetVisibleEvent { visible: true });
        }
    }
}

fn handle_sketch_canvas_visibility(
    mut events: MessageReader<CadSketchCanvasSetVisibleEvent>,
    mut state: ResMut<CadSketchUiState>,
) {
    for e in events.read() {
        state.force_open = e.visible;
        state.visible = e.visible;
        if e.visible {
            state.dismissed_for = None;
        } else {
            state.dismissed_for = state.part_entity.take();
        }
    }
}

/// Esc dismisses the sketch panel — but only when nothing more
/// urgent owns the keypress (DRAFTING_UX.md Law 1 ordering: text
/// field blur first, armed modal-tool cancel second, topmost panel
/// close third). Without the guards, one Esc press would cancel a
/// mate pick AND close the panel simultaneously.
fn close_sketch_canvas_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<CadSketchUiState>,
    active_tool: Option<Res<crate::modal_tool::ActiveModalTool>>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
) {
    if !keys.just_pressed(KeyCode::Escape) || !state.visible {
        return;
    }
    if ui_focus.as_ref().map(|f| f.text_input_focused).unwrap_or(false) {
        return;
    }
    if active_tool.as_ref().map(|t| t.is_active()).unwrap_or(false) {
        return;
    }
    state.force_open = false;
    state.visible = false;
    state.dismissed_for = state.part_entity.take();
}

fn update_sketch_ui_state(
    mut state: ResMut<CadSketchUiState>,
    selection: Option<Res<SelectionSyncManager>>,
    cad_q: Query<(Entity, &CadPart, Option<&Name>, Option<&CadPartStatus>)>,
    instances: Query<(Entity, &Instance)>,
    display_unit: Option<Res<eustress_common::units::DisplayUnit>>,
) {
    // Sketch coordinates are meters on disk; the panel shows them in
    // the status-bar display unit like every other length in Studio
    // (DRAFTING_UX.md Law 5).
    let du = display_unit
        .map(|d| d.0)
        .unwrap_or(eustress_common::units::ENGINE_NATIVE_UNIT);
    let cv = |v: f64| eustress_common::units::convert(v, eustress_common::units::ENGINE_NATIVE_UNIT, du);
    let sym = du.symbol();
    // Resolve selected CadPart
    let selected_cad = selection.as_ref().and_then(|sel| {
        let ids = sel.0.read().get_selected();
        for id in ids {
            if let Some((e, cad, name, status)) = cad_q.iter().find(|(e, ..)| {
                format!("{}v{}", e.index(), e.generation()) == id
            }) {
                return Some((e, cad, name, status));
            }
            // Also match by selection even if query order differs
            let _ = instances;
        }
        None
    });

    let Some((entity, cad, name, status)) = selected_cad else {
        if !state.force_open {
            state.visible = false;
            state.part_entity = None;
            state.entities.clear();
            state.constraints.clear();
            state.selected_a = None;
            state.selected_b = None;
        }
        return;
    };

    // Selecting a different part lifts the per-part dismissal and
    // drops any entity picks — indices are only meaningful within
    // the sketch they were picked from.
    if state.dismissed_for.is_some() && state.dismissed_for != Some(entity) {
        state.dismissed_for = None;
    }
    if state.part_entity != Some(entity) {
        state.selected_a = None;
        state.selected_b = None;
    }
    let dismissed = !state.force_open && state.dismissed_for == Some(entity);

    let Ok(tree) = parse_tree(&cad.tree_toml) else {
        state.visible = !dismissed;
        state.part_entity = Some(entity);
        state.part_name = name.map(|n| n.as_str().to_string()).unwrap_or_else(|| "CadPart".into());
        state.sketch_name = "(parse error)".into();
        state.solve_status = status.map(|s| s.message.clone()).unwrap_or_default();
        state.entities.clear();
        state.constraints.clear();
        return;
    };

    let first_sketch = tree.entries.iter().find_map(|e| match e {
        eustress_cad::FeatureEntry::Sketch { name, body } if !e.is_suppressed() => {
            Some((name.clone(), body))
        }
        _ => None,
    });

    let Some((sk_name, sk)) = first_sketch else {
        if !state.force_open {
            state.visible = false;
        }
        state.part_entity = Some(entity);
        state.entities.clear();
        state.constraints.clear();
        state.sketch_name = "(no sketch)".into();
        return;
    };

    state.visible = !dismissed;
    state.part_entity = Some(entity);
    state.part_name = name.map(|n| n.as_str().to_string()).unwrap_or_else(|| "CadPart".into());
    state.sketch_name = sk_name;
    state.solve_status = status
        .map(|s| {
            if s.ok {
                format!("✓ {}", s.message)
            } else {
                format!("✗ {}", s.message)
            }
        })
        .unwrap_or_else(|| "ready — add constraints + Solve".into());

    state.entities = sk
        .entities
        .iter()
        .enumerate()
        .map(|(i, ent)| {
            let (kind, summary) = match ent {
                eustress_cad::SketchEntity::Line { p1, p2 } => (
                    "line".into(),
                    format!("({:.2},{:.2})→({:.2},{:.2}) {sym}", cv(p1[0]), cv(p1[1]), cv(p2[0]), cv(p2[1])),
                ),
                eustress_cad::SketchEntity::Rectangle { p1, p2 } => (
                    "rect".into(),
                    format!("({:.2},{:.2})–({:.2},{:.2}) {sym}", cv(p1[0]), cv(p1[1]), cv(p2[0]), cv(p2[1])),
                ),
                eustress_cad::SketchEntity::Circle { center, radius } => (
                    "circle".into(),
                    format!("c=({:.2},{:.2}) r={:.2} {sym}", cv(center[0]), cv(center[1]), cv(*radius)),
                ),
                eustress_cad::SketchEntity::Arc { center, radius, .. } => (
                    "arc".into(),
                    format!("c=({:.2},{:.2}) r={:.2} {sym}", cv(center[0]), cv(center[1]), cv(*radius)),
                ),
                eustress_cad::SketchEntity::Point { p } => {
                    ("point".into(), format!("({:.2},{:.2}) {sym}", cv(p[0]), cv(p[1])))
                }
                eustress_cad::SketchEntity::Construction { p1, p2 } => (
                    "construction".into(),
                    format!("({:.2},{:.2})→({:.2},{:.2}) {sym}", cv(p1[0]), cv(p1[1]), cv(p2[0]), cv(p2[1])),
                ),
            };
            (i, kind, summary)
        })
        .collect();

    state.constraints = sk
        .constraints
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let detail = match c.e2 {
                Some(e2) => format!("e{} · e{}", c.e1, e2),
                None => format!("e{}", c.e1),
            };
            (i, format!("{:?}", c.kind).to_lowercase(), detail)
        })
        .collect();

    // Drop picks that no longer name a real row (constraint just
    // added a row, Solve reordered nothing but a manual TOML edit
    // could shrink the list) — a stale index would silently target
    // the wrong entity on the next constraint click.
    let n = state.entities.len();
    if state.selected_a.is_some_and(|i| i >= n) {
        state.selected_a = None;
    }
    if state.selected_b.is_some_and(|i| i >= n) {
        state.selected_b = None;
    }
}

fn handle_cad_add_constraint(
    mut events: MessageReader<CadAddConstraintEvent>,
    mut query: Query<(
        &mut CadPart,
        Option<&crate::space::instance_loader::InstanceFile>,
    )>,
    mut notifications: Option<ResMut<NotificationManager>>,
    mut undo: Option<ResMut<crate::undo::UndoStack>>,
) {
    use eustress_cad::{FeatureEntry, SketchConstraint};
    for event in events.read() {
        let Ok((mut cad, inst_file)) = query.get_mut(event.entity) else {
            continue;
        };
        let mut tree = match parse_tree(&cad.tree_toml) {
            Ok(t) => t,
            Err(e) => {
                if let Some(ref mut n) = notifications {
                    n.warning(format!("Add constraint: parse error: {e}"));
                }
                continue;
            }
        };
        let mut applied = false;
        for entry in &mut tree.entries {
            if let FeatureEntry::Sketch { body, .. } = entry {
                if event.e1 >= body.entities.len() {
                    continue;
                }
                if let Some(e2) = event.e2 {
                    if e2 >= body.entities.len() {
                        continue;
                    }
                }
                body.constraints.push(SketchConstraint {
                    kind: event.kind,
                    e1: event.e1,
                    e2: event.e2,
                });
                // Immediately solve this sketch.
                match eustress_cad::solve_sketch(body, &tree.variables) {
                    Ok(report) => {
                        eustress_cad::apply_solve(body, &report);
                        if let Some(ref mut n) = notifications {
                            n.info(format!(
                                "Constraint {:?} added — {:?} r={:.2e}",
                                event.kind, report.status, report.residual_norm
                            ));
                        }
                    }
                    Err(e) => {
                        if let Some(ref mut n) = notifications {
                            n.warning(format!(
                                "Constraint {:?} added but solve failed: {e}",
                                event.kind
                            ));
                        }
                    }
                }
                applied = true;
                break; // first sketch only
            }
        }
        if !applied {
            if let Some(ref mut n) = notifications {
                n.warning("Add constraint: no suitable sketch/entities");
            }
            continue;
        }
        match tree_to_toml(&tree) {
            Ok(s) => {
                if let Err(e) = write_features_toml(inst_file, &s) {
                    if let Some(ref mut n) = notifications {
                        n.warning(format!("Add constraint: {e}"));
                    }
                }
                if let Some(ref mut u) = undo {
                    if s != cad.tree_toml {
                        let verb = format!("Add {:?} constraint", event.kind);
                        u.push_labeled(
                            verb.clone(),
                            crate::undo::Action::CadTreeEdit {
                                entity_bits: event.entity.to_bits(),
                                old_toml: cad.tree_toml.clone(),
                                new_toml: s.clone(),
                                verb,
                            },
                        );
                    }
                }
                cad.tree_toml = s;
            }
            Err(e) => {
                if let Some(ref mut n) = notifications {
                    n.warning(format!("Add constraint: serialize failed: {e} — change lost"));
                }
            }
        }
    }
}

fn handle_cad_solve_sketch(
    mut events: MessageReader<CadSolveSketchEvent>,
    mut query: Query<(
        &mut CadPart,
        Option<&crate::space::instance_loader::InstanceFile>,
    )>,
    mut notifications: Option<ResMut<NotificationManager>>,
    mut undo: Option<ResMut<crate::undo::UndoStack>>,
) {
    for event in events.read() {
        let Ok((mut cad, inst_file)) = query.get_mut(event.entity) else {
            if let Some(ref mut n) = notifications {
                n.warning("Solve Sketch: select a CadPart");
            }
            continue;
        };
        let mut tree = match parse_tree(&cad.tree_toml) {
            Ok(t) => t,
            Err(e) => {
                if let Some(ref mut n) = notifications {
                    n.warning(format!("Solve: parse error: {e}"));
                }
                continue;
            }
        };
        let mut reports = Vec::new();
        for entry in &mut tree.entries {
            if let eustress_cad::FeatureEntry::Sketch { name, body } = entry {
                match eustress_cad::solve_sketch(body, &tree.variables) {
                    Ok(report) => {
                        eustress_cad::apply_solve(body, &report);
                        reports.push(format!(
                            "{name}: {:?} r={:.2e} dof={}",
                            report.status, report.residual_norm, report.free_dof
                        ));
                    }
                    Err(e) => reports.push(format!("{name}: err {e}")),
                }
            }
        }
        match tree_to_toml(&tree) {
            Ok(s) => {
                if let Err(e) = write_features_toml(inst_file, &s) {
                    if let Some(ref mut n) = notifications {
                        n.warning(format!("Solve: {e}"));
                    }
                }
                if let Some(ref mut u) = undo {
                    if s != cad.tree_toml {
                        u.push_labeled(
                            "Solve sketch",
                            crate::undo::Action::CadTreeEdit {
                                entity_bits: event.entity.to_bits(),
                                old_toml: cad.tree_toml.clone(),
                                new_toml: s.clone(),
                                verb: "Solve sketch".into(),
                            },
                        );
                    }
                }
                cad.tree_toml = s;
                if let Some(ref mut n) = notifications {
                    n.success(format!("Sketch solved — {}", reports.join("; ")));
                }
            }
            Err(e) => {
                if let Some(ref mut n) = notifications {
                    n.warning(format!("Solve serialize: {e}"));
                }
            }
        }
    }
}

fn handle_cad_tree_ops(
    mut events: MessageReader<CadTreeOpEvent>,
    mut query: Query<(
        &mut CadPart,
        Option<&crate::space::instance_loader::InstanceFile>,
    )>,
    mut notifications: Option<ResMut<NotificationManager>>,
    mut undo: Option<ResMut<crate::undo::UndoStack>>,
) {
    use eustress_cad::FeatureTree;
    for event in events.read() {
        let Ok((mut cad, inst_file)) = query.get_mut(event.entity) else {
            continue;
        };
        let mut tree = match parse_tree(&cad.tree_toml) {
            Ok(t) => t,
            Err(e) => {
                if let Some(ref mut n) = notifications {
                    n.warning(format!("Tree op: parse error: {e}"));
                }
                continue;
            }
        };
        // History verb names the feature the op touched, pre-mutation
        // (Delete removes the entry we'd otherwise read).
        let name_at = |tree: &FeatureTree, ix: usize| {
            tree.entries
                .get(ix)
                .map(|e| e.name().to_string())
                .unwrap_or_else(|| format!("#{ix}"))
        };
        let verb = match &event.op {
            CadTreeOp::Suppress { index } => format!("Suppress {}", name_at(&tree, *index)),
            CadTreeOp::Unsuppress { index } => format!("Unsuppress {}", name_at(&tree, *index)),
            CadTreeOp::Reorder { from, to } => {
                format!("Reorder {} → {to}", name_at(&tree, *from))
            }
            CadTreeOp::Delete { index } => format!("Delete {}", name_at(&tree, *index)),
        };
        let result = match &event.op {
            CadTreeOp::Suppress { index } => tree.suppress(*index),
            CadTreeOp::Unsuppress { index } => tree.unsuppress(*index),
            CadTreeOp::Reorder { from, to } => {
                if tree.reorder(*from, *to) {
                    Ok(())
                } else {
                    Err("reorder failed".into())
                }
            }
            CadTreeOp::Delete { index } => {
                if tree.delete(*index) {
                    Ok(())
                } else {
                    Err("delete failed".into())
                }
            }
        };
        if let Err(e) = result {
            if let Some(ref mut n) = notifications {
                n.warning(format!("Tree op failed: {e}"));
            }
            continue;
        }
        match tree_to_toml(&tree) {
            Ok(s) => {
                if let Err(e) = write_features_toml(inst_file, &s) {
                    if let Some(ref mut n) = notifications {
                        n.warning(format!("Tree op: {e}"));
                    }
                }
                if let Some(ref mut u) = undo {
                    if s != cad.tree_toml {
                        u.push_labeled(
                            verb.clone(),
                            crate::undo::Action::CadTreeEdit {
                                entity_bits: event.entity.to_bits(),
                                old_toml: cad.tree_toml.clone(),
                                new_toml: s.clone(),
                                verb,
                            },
                        );
                    }
                }
                cad.tree_toml = s;
                if let Some(ref mut n) = notifications {
                    n.info("Feature tree updated — regenerating");
                }
            }
            Err(e) => {
                if let Some(ref mut n) = notifications {
                    n.warning(format!("serialize tree: {e}"));
                }
            }
        }
    }
}

// ============================================================================
// Variable patch
// ============================================================================

fn handle_cad_set_variable(
    mut events: MessageReader<CadSetVariableEvent>,
    mut query: Query<(
        &mut CadPart,
        Option<&crate::space::instance_loader::InstanceFile>,
    )>,
    mut notifications: Option<ResMut<NotificationManager>>,
    mut undo: Option<ResMut<crate::undo::UndoStack>>,
) {
    // One undo entry per entity per frame — a single resize gesture
    // fires up to three variable events (length/width/height); the
    // user made ONE edit and gets ONE Ctrl+Z.
    let mut edits: std::collections::HashMap<Entity, (String, Vec<String>)> =
        std::collections::HashMap::new();

    for event in events.read() {
        let Ok((mut cad, inst_file)) = query.get_mut(event.entity) else {
            if let Some(ref mut n) = notifications {
                n.warning("CadPart: entity has no CadPart component");
            }
            continue;
        };
        match set_variable_in_toml(&cad.tree_toml, &event.name, &event.value) {
            Ok(new_toml) => {
                edits
                    .entry(event.entity)
                    .or_insert_with(|| (cad.tree_toml.clone(), Vec::new()))
                    .1
                    .push(event.name.clone());
                // Persist to features.toml when the part is file-backed so
                // git / MCP / reload see the same values.
                if let Some(inst) = inst_file {
                    if let Some(parent) = inst.toml_path.parent() {
                        let feat = parent.join("features.toml");
                        if let Err(e) = std::fs::write(&feat, &new_toml) {
                            warn!("📐 failed to persist features.toml: {e}");
                        }
                    }
                }
                cad.tree_toml = new_toml;
                // Changed<CadPart> triggers regenerate.
                if let Some(ref mut n) = notifications {
                    n.info(format!(
                        "CadPart: {} = {} — regenerating",
                        event.name, event.value
                    ));
                }
            }
            Err(e) => {
                if let Some(ref mut n) = notifications {
                    n.warning(format!("CadPart variable set failed: {e}"));
                }
            }
        }
    }

    if let Some(ref mut u) = undo {
        for (entity, (old_toml, mut names)) in edits {
            let Ok((cad, _)) = query.get(entity) else { continue };
            if cad.tree_toml == old_toml {
                continue; // no-op edit — don't pollute history
            }
            names.sort();
            names.dedup();
            let verb = format!("Set {}", names.join(", "));
            u.push_labeled(
                verb.clone(),
                crate::undo::Action::CadTreeEdit {
                    entity_bits: entity.to_bits(),
                    old_toml,
                    new_toml: cad.tree_toml.clone(),
                    verb,
                },
            );
        }
    }
}

/// Patch `[variables]` table in feature-tree TOML. Adds the key if missing.
fn set_variable_in_toml(toml_src: &str, name: &str, value: &str) -> Result<String, String> {
    let mut tree = parse_tree(toml_src).map_err(|e| e.to_string())?;
    tree.variables.insert(name.to_string(), value.to_string());
    tree_to_toml(&tree).map_err(|e| e.to_string())
}

// ============================================================================
// Regenerate
// ============================================================================

fn regenerate_cad_parts(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut query: Query<
        (
            Entity,
            &CadPart,
            Option<&mut BasePart>,
            Option<&mut Transform>,
            Option<&mut Collider>,
        ),
        Changed<CadPart>,
    >,
) {
    // Note: insert path already evaluates once. Changed fires on first
    // insert too (component added counts as changed in Bevy) — a second
    // eval is fine and keeps the single code path for mesh binding.
    for (entity, cad, base_part, transform, collider) in query.iter_mut() {
        let fail = |commands: &mut Commands, entity: Entity, msg: String| {
            commands.entity(entity).insert(CadPartStatus {
                ok: false,
                message: msg,
            });
        };

        let tree = match parse_tree(&cad.tree_toml) {
            Ok(t) => t,
            Err(e) => {
                fail(&mut commands, entity, format!("parse error: {e}"));
                continue;
            }
        };
        let out = match evaluate_tree(&tree) {
            Ok(o) => o,
            Err(e) => {
                fail(&mut commands, entity, format!("eval error: {e}"));
                continue;
            }
        };
        let Some(eval_mesh) = out.mesh.filter(|m| !m.indices.is_empty()) else {
            fail(&mut commands, entity, "no mesh produced".into());
            continue;
        };

        let (min, max) = mesh_bounds(&eval_mesh);
        let center_local = (min + max) * 0.5;
        let size = (max - min).max(Vec3::splat(0.01));
        let mut bevy_mesh = eval_mesh_to_bevy(&eval_mesh);
        bevy_mesh = translate_mesh(bevy_mesh, -center_local);
        let new_handle = meshes.add(bevy_mesh);
        commands.entity(entity).insert(Mesh3d(new_handle));

        // Respect can_collide — the loader's documented rule: no
        // broadphase collider for decorative parts (instance_loader
        // only attaches one when can_collide is true).
        let wants_collider = base_part.as_ref().map(|bp| bp.can_collide).unwrap_or(true);
        if let Some(mut bp) = base_part {
            bp.size = size;
        }
        if let Some(mut tf) = transform {
            tf.scale = Vec3::ONE;
        }
        let half = size * 0.5;
        if wants_collider {
            if let Some(mut col) = collider {
                *col = Collider::cuboid(half.x.max(0.001), half.y.max(0.001), half.z.max(0.001));
            } else {
                commands.entity(entity).insert(Collider::cuboid(
                    half.x.max(0.001),
                    half.y.max(0.001),
                    half.z.max(0.001),
                ));
            }
        } else if collider.is_some() {
            commands.entity(entity).remove::<Collider>();
        }

        commands.entity(entity).insert(CadPartStatus {
            ok: true,
            message: format_status(&out.entry_status, true),
        });
        debug!("📐 CadPart regenerated {:?}", entity);
    }
}

fn format_status(entries: &[eustress_cad::EntryStatus], ok: bool) -> String {
    if !ok {
        return "failed".into();
    }
    let bad: Vec<_> = entries.iter().filter(|e| !e.ok).map(|e| e.name.as_str()).collect();
    if bad.is_empty() {
        format!("{} features ok", entries.len())
    } else {
        format!("issues: {}", bad.join(", "))
    }
}

// ============================================================================
// Templates (meter-native)
// ============================================================================

fn template_label(t: CadTemplate) -> &'static str {
    match t {
        CadTemplate::Plate => "CadPlate",
        CadTemplate::Box => "CadBox",
        CadTemplate::Cylinder => "CadCylinder",
        CadTemplate::PlateWithHole => "CadPlateHole",
        CadTemplate::LBracket => "CadLBracket",
        CadTemplate::ConstrainedFrame => "CadFrame",
        CadTemplate::ShelledBox => "CadShell",
    }
}

fn template_toml(t: CadTemplate) -> String {
    use eustress_cad::templates as tpl;
    match t {
        CadTemplate::Plate => tpl::PLATE_TOML.into(),
        CadTemplate::Box => tpl::BOX_TOML.into(),
        CadTemplate::Cylinder => tpl::CYLINDER_TOML.into(),
        CadTemplate::PlateWithHole => tpl::PLATE_HOLE_TOML.into(),
        CadTemplate::LBracket => tpl::L_BRACKET_TOML.into(),
        CadTemplate::ConstrainedFrame => tpl::CONSTRAINED_FRAME_TOML.into(),
        CadTemplate::ShelledBox => tpl::SHELLED_BOX_TOML.into(),
    }
}


// ============================================================================
// Export GLB
// ============================================================================

fn handle_cad_export_glb(
    mut events: MessageReader<CadExportGlbEvent>,
    query: Query<(
        &CadPart,
        Option<&Name>,
        Option<&crate::space::instance_loader::InstanceFile>,
    )>,
    mut notifications: Option<ResMut<NotificationManager>>,
) {
    for event in events.read() {
        let Ok((cad, name, inst_file)) = query.get(event.entity) else {
            if let Some(ref mut n) = notifications {
                n.warning("Export GLB: select a CadPart first");
            }
            continue;
        };
        let tree = match parse_tree(&cad.tree_toml) {
            Ok(t) => t,
            Err(e) => {
                if let Some(ref mut n) = notifications {
                    n.warning(format!("Export GLB: parse error: {e}"));
                }
                continue;
            }
        };
        let out = match evaluate_tree(&tree) {
            Ok(o) => o,
            Err(e) => {
                if let Some(ref mut n) = notifications {
                    n.warning(format!("Export GLB: eval error: {e}"));
                }
                continue;
            }
        };
        let Some(mesh) = out.mesh.filter(|m| !m.indices.is_empty()) else {
            if let Some(ref mut n) = notifications {
                n.warning("Export GLB: no mesh to export");
            }
            continue;
        };

        let label = name
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|| "CadPart".into());
        let path = event.path.clone().unwrap_or_else(|| {
            if let Some(inst) = inst_file {
                if let Some(parent) = inst.toml_path.parent() {
                    return parent.join(format!("{label}.glb"));
                }
            }
            std::path::PathBuf::from("exports").join(format!("{label}.glb"))
        });
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let extras = serde_json::json!({
            "eustress": {
                "kind": "CadPart",
                "generator": "eustress-cad",
                "variables": tree.variables,
                "features": out.entry_status.iter().map(|s| {
                    serde_json::json!({ "name": s.name, "ok": s.ok, "message": s.message })
                }).collect::<Vec<_>>(),
            }
        });

        match eustress_cad::write_glb(&path, &mesh, Some(extras.clone())) {
            Ok(()) => {
                // Round-trip into the Space asset library so Toolbox / Insert
                // can pick the mesh up as a standard GLB part.
                let library_note = publish_glb_to_asset_library(&label, &path, &tree.variables);
                if let Some(ref mut n) = notifications {
                    n.success(format!(
                        "Exported GLB → {}{}",
                        path.display(),
                        library_note.as_deref().unwrap_or("")
                    ));
                }
                info!("📐 Exported CadPart GLB → {:?} {:?}", path, library_note);
            }
            Err(e) => {
                if let Some(ref mut n) = notifications {
                    n.warning(format!("Export GLB failed: {e}"));
                }
            }
        }
    }
}

/// Copy exported GLB into `Workspace/Assets/Cad/{label}.glb` + a small
/// `_instance.toml` stub so the Asset Manager / Toolbox can discover it.
fn publish_glb_to_asset_library(
    label: &str,
    source_glb: &std::path::Path,
    variables: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let space_root = crate::space::default_space_root();
    let lib_dir = space_root.join("Workspace").join("Assets").join("Cad");
    std::fs::create_dir_all(&lib_dir).ok()?;
    let safe: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let dest = lib_dir.join(format!("{safe}.glb"));
    std::fs::copy(source_glb, &dest).ok()?;
    // Sidecar TOML for asset browser metadata.
    let meta = format!(
        r#"[metadata]
class_name = "Part"
name = "{safe}"
source = "cad_export"

[asset]
mesh = "Assets/Cad/{safe}.glb"
scene = "Scene0"

[cad]
variables = {:?}
"#,
        variables
    );
    let _ = std::fs::write(lib_dir.join(format!("{safe}.toml")), meta);
    Some(format!(" · library Assets/Cad/{safe}.glb"))
}

// ============================================================================
// Mesh helpers (shared shape with csg.rs)
// ============================================================================

pub fn eval_mesh_to_bevy(eval: &EvalMesh) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, eval.positions.clone());
    let normals = if eval.normals.len() == eval.positions.len() {
        eval.normals.clone()
    } else {
        vec![[0.0, 1.0, 0.0]; eval.positions.len()]
    };
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    if eval.uvs.len() == eval.positions.len() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, eval.uvs.clone());
    } else {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; eval.positions.len()]);
    }
    mesh.insert_indices(Indices::U32(eval.indices.clone()));
    mesh
}

fn mesh_bounds(eval: &EvalMesh) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for p in &eval.positions {
        let v = Vec3::new(p[0], p[1], p[2]);
        min = min.min(v);
        max = max.max(v);
    }
    if !min.is_finite() {
        (Vec3::ZERO, Vec3::ONE)
    } else {
        (min, max)
    }
}

fn translate_mesh(mut mesh: Mesh, offset: Vec3) -> Mesh {
    if let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for p in positions.iter_mut() {
            p[0] += offset.x;
            p[1] += offset.y;
            p[2] += offset.z;
        }
    }
    mesh
}
