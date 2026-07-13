//! # CSG (Constructive Solid Geometry) — Drafting Boolean tools
//!
//! Wired from the Drafting ribbon Boolean group:
//! - **Union / Subtract / Intersect** — real truck booleans via
//!   `eustress-cad::boolean_oriented_solids` for Block / Cylinder
//!   BaseParts (exact) and Ball / Cone / Wedge (approximated as
//!   cube / straight cylinder / block until the kernel grows the
//!   exact shapes — the result toast discloses which approximations
//!   applied). Result spawns as a new Part with the tessellated mesh;
//!   source parts are despawned (undo via SpawnFolders is not yet
//!   recorded — follow-up).
//! - **Separate** — dissolve a selected Model (same semantics as Ungroup).
//!
//! Falls back to a non-destructive Model group when the kernel cannot
//! produce a solid (unsupported shapes, boolean failure) so the button
//! never silently no-ops.

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::asset::RenderAssetUsages;
use avian3d::prelude::{Collider, RigidBody};

use eustress_cad::{
    boolean_oriented_solids, BooleanOp, OrientedShape, OrientedSolid, EvalMesh,
};
use eustress_common::classes::{
    BasePart, ClassName, Instance, Material, Part, PartType,
};
use eustress_common::attributes::{Attributes, Tags};

use crate::keybindings::Action;
use crate::notifications::NotificationManager;
use crate::rendering::PartEntity;
use crate::selection_sync::SelectionSyncManager;
use crate::ui::MenuActionEvent;

// ============================================================================
// Plugin
// ============================================================================

pub struct CsgPlugin;

impl Plugin for CsgPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, handle_csg_actions);
    }
}

// ============================================================================
// Handler
// ============================================================================

fn handle_csg_actions(
    mut events: MessageReader<MenuActionEvent>,
    mut commands: Commands,
    selection: Option<Res<SelectionSyncManager>>,
    parts: Query<(
        Entity,
        &Instance,
        Option<&GlobalTransform>,
        Option<&BasePart>,
        Option<&Part>,
        Option<&Children>,
        Option<&ChildOf>,
    )>,
    global_q: Query<&GlobalTransform>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut notifications: ResMut<NotificationManager>,
    instance_files: Query<&crate::space::instance_loader::InstanceFile>,
    mut undo: Option<ResMut<crate::undo::UndoStack>>,
    mut registry: Option<ResMut<crate::space::SpaceFileRegistry>>,
) {
    let Some(selection) = selection else {
        return;
    };

    for event in events.read() {
        match event.action {
            Action::CSGUnion => {
                run_boolean(
                    BooleanOp::Union,
                    "Union",
                    &mut commands,
                    &selection,
                    &parts,
                    &mut meshes,
                    &mut materials,
                    &mut notifications,
                    &instance_files,
                    &mut undo,
                    &mut registry,
                );
            }
            Action::CSGNegate => {
                run_boolean(
                    BooleanOp::Difference,
                    "Subtract",
                    &mut commands,
                    &selection,
                    &parts,
                    &mut meshes,
                    &mut materials,
                    &mut notifications,
                    &instance_files,
                    &mut undo,
                    &mut registry,
                );
            }
            Action::CSGIntersect => {
                run_boolean(
                    BooleanOp::Intersect,
                    "Intersect",
                    &mut commands,
                    &selection,
                    &parts,
                    &mut meshes,
                    &mut materials,
                    &mut notifications,
                    &instance_files,
                    &mut undo,
                    &mut registry,
                );
            }
            Action::CSGSeparate => {
                separate_models(
                    &mut commands,
                    &selection,
                    &parts,
                    &global_q,
                    &mut notifications,
                );
            }
            _ => {}
        }
    }
}

fn entity_id_str(e: Entity) -> String {
    format!("{}v{}", e.index(), e.generation())
}

fn selected_set(selection: &SelectionSyncManager) -> std::collections::HashSet<String> {
    selection.0.read().get_selected().into_iter().collect()
}

fn run_boolean(
    op: BooleanOp,
    label: &str,
    commands: &mut Commands,
    selection: &SelectionSyncManager,
    parts: &Query<(
        Entity,
        &Instance,
        Option<&GlobalTransform>,
        Option<&BasePart>,
        Option<&Part>,
        Option<&Children>,
        Option<&ChildOf>,
    )>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    notifications: &mut NotificationManager,
    instance_files: &Query<&crate::space::instance_loader::InstanceFile>,
    undo: &mut Option<ResMut<crate::undo::UndoStack>>,
    registry: &mut Option<ResMut<crate::space::SpaceFileRegistry>>,
) {
    let selected = selected_set(selection);
    if selected.len() < 2 {
        notifications.warning(format!(
            "{label}: select ≥2 parts first (have {})",
            selected.len()
        ));
        return;
    }

    // Collect operable BaseParts, preserving selection order when possible
    // by iterating the selection manager's order.
    let ordered_ids = selection.0.read().get_selected();
    let mut operands: Vec<(Entity, OrientedSolid, [f32; 4], Material)> = Vec::new();
    // Shape approximations the kernel makes (Ball has no BRep sphere
    // yet, Cone has no taper, Wedge folds to its bounding block) —
    // surfaced in the result toast so cuts that come out wrong-shaped
    // are explained, not mysterious.
    let mut approximations: std::collections::BTreeSet<&'static str> =
        std::collections::BTreeSet::new();

    for id in &ordered_ids {
        let Some((entity, _inst, gt, bp, part, _ch, _co)) =
            parts.iter().find(|(e, ..)| &entity_id_str(*e) == id)
        else {
            continue;
        };
        let Some(bp) = bp else { continue };
        let Some(gt) = gt else { continue };

        let (scale, mut rot, trans) = gt.to_scale_rotation_translation();
        // Effective size includes transform.scale (glb parts store size in
        // BasePart and also scale the unit mesh).
        let size = bp.size * scale;
        let part_type = part.map(|p| p.shape).unwrap_or(PartType::Block);
        let shape = match part_type {
            PartType::Ball => {
                approximations.insert("Ball→cube");
                OrientedShape::Ball {
                    radius: (size.x.max(size.y).max(size.z) * 0.5) as f64,
                }
            }
            PartType::Cylinder | PartType::Cone => {
                if matches!(part_type, PartType::Cone) {
                    approximations.insert("Cone→cylinder");
                }
                // Engine cylinders point along local +Y (the Avian/Bevy
                // convention used everywhere: spawn.rs, scale_tool.rs,
                // colliders); OrientedShape::Cylinder extrudes along
                // local +Z. Pre-rotate so shape-Z lands on part-Y —
                // without this every cylinder boolean is 90° off.
                rot = rot * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
                OrientedShape::Cylinder {
                    radius: (size.x.max(size.z) * 0.5) as f64,
                    height: size.y as f64,
                }
            }
            PartType::Wedge | PartType::CornerWedge => {
                approximations.insert("Wedge→block");
                OrientedShape::Block {
                    size: [size.x as f64, size.y as f64, size.z as f64],
                }
            }
            _ => OrientedShape::Block {
                size: [size.x as f64, size.y as f64, size.z as f64],
            },
        };

        let srgba = bp.color.to_srgba();
        let color = [srgba.red, srgba.green, srgba.blue, srgba.alpha];
        operands.push((
            entity,
            OrientedSolid {
                shape,
                translation: [trans.x as f64, trans.y as f64, trans.z as f64],
                rotation_xyzw: [rot.x as f64, rot.y as f64, rot.z as f64, rot.w as f64],
            },
            color,
            bp.material,
        ));
    }

    if operands.len() < 2 {
        // Fall back: group any selected instances into a Model so the
        // button always does *something* discoverable.
        notifications.warning(format!(
            "{label}: need ≥2 BaseParts with transforms — grouping selection as Model instead"
        ));
        group_selection_as_model(commands, selection, parts, label, notifications);
        return;
    }

    let solids: Vec<OrientedSolid> = operands.iter().map(|(_, s, ..)| s.clone()).collect();
    match boolean_oriented_solids(op, &solids) {
        Ok(eval_mesh) if !eval_mesh.indices.is_empty() => {
            let (color, mat) = (operands[0].2, operands[0].3);
            let source_entities: Vec<Entity> = operands.iter().map(|(e, ..)| *e).collect();

            // Approximate AABB from mesh for collider + BasePart.size
            let (min, max) = mesh_bounds(&eval_mesh);
            let center = (min + max) * 0.5;
            let size = (max - min).max(Vec3::splat(0.01));

            let bevy_mesh = eval_mesh_to_bevy(&eval_mesh);
            // Translate mesh so its local origin is the AABB center
            // (entity Transform sits at center).
            let bevy_mesh = translate_mesh(bevy_mesh, -center);
            let mesh_handle = meshes.add(bevy_mesh);

            let (roughness, metallic, reflectance) = mat.pbr_params();
            let material_handle = materials.add(StandardMaterial {
                base_color: Color::srgba(color[0], color[1], color[2], color[3]),
                perceptual_roughness: roughness,
                metallic,
                reflectance,
                ..default()
            });

            // Persist the result so it survives a Space reload, and so
            // undo/redo have file-rename semantics: `result.glb`
            // (mesh normalized to unit scale — the loader multiplies
            // by transform.scale, which carries the real size, per the
            // "glb parts store size in BasePart and scale the unit
            // mesh" convention) + `_instance.toml` beside it.
            let uuid = eustress_common::instance_create::fresh_uuid_for_create();
            let space_root = crate::space::default_space_root();
            let workspace = space_root.join("Workspace");
            let _ = std::fs::create_dir_all(&workspace);
            let folder_name = crate::space::instance_loader::unique_entity_name(
                &workspace,
                &format!("{label}Result"),
            );
            let result_dir = workspace.join(&folder_name);
            let persisted = persist_csg_result(
                &result_dir, &folder_name, &uuid, &eval_mesh, center, size, color, mat,
            );
            if let Err(ref e) = persisted {
                warn!("🔨 CSG {label}: result persistence failed ({e}) — part will be session-only");
            }

            let name = folder_name.clone();
            let instance = Instance {
                name: name.clone(),
                class_name: ClassName::Part,
                archivable: true,
                id: 0,
                ai: false,
                uuid,
            };
            let mut bp = BasePart::default();
            bp.size = size;
            bp.cframe = Transform::from_translation(center);
            bp.color = Color::srgba(color[0], color[1], color[2], color[3]);
            bp.material = mat;
            bp.can_collide = true;
            bp.anchored = true;

            let part = Part {
                shape: PartType::Block,
            };

            let half = size * 0.5;
            let entity = commands
                .spawn((
                    Mesh3d(mesh_handle),
                    MeshMaterial3d(material_handle),
                    Transform::from_translation(center),
                    instance,
                    bp,
                    part,
                    Collider::cuboid(half.x, half.y, half.z),
                    RigidBody::Static,
                    Name::new(name.clone()),
                    PartEntity {
                        part_id: String::new(),
                    },
                    Attributes::new(),
                    Tags::new(),
                ))
                .id();
            let part_id = entity_id_str(entity);
            commands.entity(entity).insert(PartEntity { part_id: part_id.clone() });

            if persisted.is_ok() {
                // Link entity ↔ folder so the file watcher doesn't
                // double-spawn, saves round-trip, and delete/undo see
                // a normal disk-backed part.
                let instance_toml = result_dir.join("_instance.toml");
                commands.entity(entity).insert(
                    crate::space::instance_loader::InstanceFile {
                        toml_path: instance_toml.clone(),
                        mesh_path: result_dir.join("result.glb"),
                        name: folder_name.clone(),
                    },
                );
                if let Some(ref mut reg) = registry {
                    // Register the `_instance.toml` path, not the bare
                    // folder — the file watcher's created-file guard
                    // checks `registry.is_loaded(&event.path)` against
                    // the exact file it saw appear, so registering only
                    // the directory never actually suppressed the
                    // double-spawn this comment claims it does.
                    let modified = std::fs::metadata(&result_dir)
                        .and_then(|m| m.modified())
                        .unwrap_or_else(|_| std::time::SystemTime::now());
                    reg.register(
                        instance_toml.clone(),
                        entity,
                        crate::space::file_loader::FileMetadata {
                            path: instance_toml,
                            file_type: crate::space::file_loader::FileType::Toml,
                            service: "Workspace".to_string(),
                            name: folder_name.clone(),
                            size: 0,
                            modified,
                            children: Vec::new(),
                        },
                    );
                }
            }

            // Commit = sources leave the scene AND the disk (their
            // folders move to trash — leaving them in place would
            // resurrect the operands on the next Space reload). The
            // Batch entry makes the whole boolean one Ctrl+Z step:
            // undo trashes the result folder + restores the sources;
            // redo re-trashes the sources + restores the result.
            let trash_stamp = chrono::Utc::now().format("%Y%m%d_%H%M%S_%f").to_string();
            let trash_root = space_root.join(".eustress").join("trash").join(&trash_stamp);
            let mut source_folder_moves: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();
            let mut unbacked_sources = 0usize;
            for src in &source_entities {
                if let Ok(f) = instance_files.get(*src) {
                    if let Some(folder) = f.toml_path.parent() {
                        source_folder_moves.push((
                            folder.to_path_buf(),
                            trash_root.join(folder.file_name().unwrap_or_default()),
                        ));
                        continue;
                    }
                }
                unbacked_sources += 1;
            }
            for (orig, trash) in &source_folder_moves {
                if let Some(parent) = trash.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::rename(orig, trash) {
                    warn!("🔨 CSG {label}: failed to trash source {:?}: {e}", orig);
                }
            }

            if persisted.is_ok() && unbacked_sources == 0 {
                if let Some(ref mut u) = undo {
                    let result_trash = space_root
                        .join(".eustress")
                        .join("trash")
                        .join(format!("{trash_stamp}-result"))
                        .join(&folder_name);
                    u.push_labeled(
                        format!("{label} ({} parts)", solids.len()),
                        crate::undo::Action::Batch {
                            actions: vec![
                                crate::undo::Action::TrashEntities {
                                    paths: source_folder_moves,
                                },
                                crate::undo::Action::SpawnFolders {
                                    folders: vec![(result_dir.clone(), result_trash)],
                                },
                            ],
                        },
                    );
                }
            } else if unbacked_sources > 0 {
                notifications.warning(format!(
                    "{label}: {unbacked_sources} source part(s) had no saved folder — this boolean can't be undone"
                ));
            }

            // Despawn sources (they are replaced by the boolean result).
            for src in source_entities {
                commands.entity(src).despawn();
            }

            selection.0.write().set_selected(vec![part_id]);
            if approximations.is_empty() {
                notifications.success(format!(
                    "{label}: created solid from {} parts",
                    solids.len()
                ));
            } else {
                let notes: Vec<&str> = approximations.into_iter().collect();
                notifications.warning(format!(
                    "{label}: created solid from {} parts — approximated: {} (exact BRep shapes pending)",
                    solids.len(),
                    notes.join(", ")
                ));
            }
            info!("🔨 CSG {label}: spawned result entity {:?}", entity);
        }
        Ok(_) => {
            notifications.warning(format!(
                "{label}: boolean returned empty mesh — grouping as Model instead"
            ));
            group_selection_as_model(commands, selection, parts, label, notifications);
        }
        Err(e) => {
            warn!("🔨 CSG {label} kernel error: {e} — falling back to Model group");
            notifications.warning(format!(
                "{label} failed ({e}) — grouped selection as Model instead"
            ));
            group_selection_as_model(commands, selection, parts, label, notifications);
        }
    }
}

/// Write a boolean result to disk as a normal part folder:
/// `result.glb` (unit-normalized — the loader multiplies the mesh by
/// `transform.scale`, which carries the real size) + `_instance.toml`.
#[allow(clippy::too_many_arguments)]
fn persist_csg_result(
    dir: &std::path::Path,
    name: &str,
    uuid: &str,
    eval_mesh: &EvalMesh,
    center: Vec3,
    size: Vec3,
    color: [f32; 4],
    mat: Material,
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("mkdir: {e}"))?;

    let mut unit = eval_mesh.clone();
    for p in &mut unit.positions {
        p[0] = (p[0] - center.x) / size.x;
        p[1] = (p[1] - center.y) / size.y;
        p[2] = (p[2] - center.z) / size.z;
    }
    eustress_cad::write_glb(&dir.join("result.glb"), &unit, None)
        .map_err(|e| format!("glb: {e}"))?;

    let toml = format!(
        r#"[metadata]
class_name = "Part"
archivable = true
name = "{name}"
uuid = "{uuid}"

[transform]
position = [{}, {}, {}]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [{}, {}, {}]

[properties]
color = [{:.4}, {:.4}, {:.4}]
transparency = {:.4}
anchored = true
can_collide = true
cast_shadow = true
reflectance = 0.0
material = "{:?}"
locked = false

[asset]
mesh = "result.glb"
scene = "Scene0"
"#,
        center.x,
        center.y,
        center.z,
        size.x,
        size.y,
        size.z,
        color[0],
        color[1],
        color[2],
        1.0 - color[3],
        mat,
    );
    std::fs::write(dir.join("_instance.toml"), toml).map_err(|e| format!("toml: {e}"))
}

fn group_selection_as_model(
    commands: &mut Commands,
    selection: &SelectionSyncManager,
    parts: &Query<(
        Entity,
        &Instance,
        Option<&GlobalTransform>,
        Option<&BasePart>,
        Option<&Part>,
        Option<&Children>,
        Option<&ChildOf>,
    )>,
    label: &str,
    notifications: &mut NotificationManager,
) {
    let selected = selected_set(selection);
    let mut members: Vec<(Entity, Vec3, Quat, Vec3)> = Vec::new();
    for (e, inst, gt, ..) in parts.iter() {
        if !selected.contains(&entity_id_str(e)) {
            continue;
        }
        if inst.class_name.is_adornment() {
            continue;
        }
        let (scale, rot, trans) = gt
            .map(|g| g.to_scale_rotation_translation())
            .unwrap_or((Vec3::ONE, Quat::IDENTITY, Vec3::ZERO));
        members.push((e, trans, rot, scale));
    }
    if members.len() < 2 {
        notifications.warning(format!("{label}: need ≥2 selectable objects"));
        return;
    }
    let center = members.iter().map(|(_, t, _, _)| *t).sum::<Vec3>() / members.len() as f32;
    let model = commands
        .spawn((
            Instance {
                name: format!("{label}Group"),
                class_name: ClassName::Model,
                archivable: true,
                id: 0,
                ai: false,
                uuid: String::new(),
            },
            Transform::from_translation(center),
            Visibility::default(),
            Name::new(format!("{label}Group")),
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
    notifications.info(format!(
        "{label}: grouped {} objects into a Model",
        members.len()
    ));
}

fn separate_models(
    commands: &mut Commands,
    selection: &SelectionSyncManager,
    parts: &Query<(
        Entity,
        &Instance,
        Option<&GlobalTransform>,
        Option<&BasePart>,
        Option<&Part>,
        Option<&Children>,
        Option<&ChildOf>,
    )>,
    global_q: &Query<&GlobalTransform>,
    notifications: &mut NotificationManager,
) {
    let selected = selected_set(selection);
    if selected.is_empty() {
        notifications.warning("Separate: select a Model to dissolve");
        return;
    }

    let mut freed: Vec<String> = Vec::new();
    let mut containers = 0u32;

    for (model_e, inst, _gt, _bp, _part, children, child_of) in parts.iter() {
        if !selected.contains(&entity_id_str(model_e)) {
            continue;
        }
        if !matches!(inst.class_name, ClassName::Model | ClassName::Folder) {
            continue;
        }
        let Some(children) = children else { continue };
        let kids: Vec<Entity> = children.iter().collect();
        if kids.is_empty() {
            continue;
        }
        let grandparent = child_of.map(|c| c.0);
        for child in kids {
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
        commands.entity(model_e).despawn();
        containers += 1;
    }

    if containers == 0 {
        notifications.warning("Separate: select a Model or Folder (not individual parts)");
        return;
    }
    if !freed.is_empty() {
        selection.0.write().set_selected(freed.clone());
    }
    notifications.success(format!(
        "Separate: dissolved {containers} container(s), freed {} parts",
        freed.len()
    ));
}

// ============================================================================
// Mesh helpers
// ============================================================================

fn eval_mesh_to_bevy(eval: &EvalMesh) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, eval.positions.clone());
    // Tessellate always fills normals; fall back to unit-Y if empty.
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
