// ============================================================================
// Eustress Engine - Scale Tool
// ============================================================================
// ## Table of Contents
// 1. State & types
// 2. Plugin registration
// 3. Gizmo drawing (cube handles at face centers, camera-scaled)
// 4. Mouse interaction (per-axis and symmetric scaling)
// 5. Public helpers
// ============================================================================

#![allow(dead_code)]
#![allow(unused_variables)]

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::selection_box::SelectionBox;
use crate::gizmo_tools::TransformGizmoGroup;
use crate::math_utils::{ray_plane_intersection, ray_to_point_distance, calculate_rotated_aabb};
use crate::move_tool::Axis3d;

// ============================================================================
// 1. State & Types
// ============================================================================

#[derive(Resource, Default)]
pub struct ScaleToolState {
    pub active: bool,
    pub dragged_axis: Option<ScaleAxis>,
    pub initial_scale: Vec3,
    pub initial_position: Vec3,
    pub drag_start_pos: Vec2,
    pub initial_mouse_world: Vec3,
    pub dragged_entity: Option<Entity>,
    pub initial_scales: std::collections::HashMap<Entity, Vec3>,
    pub initial_positions: std::collections::HashMap<Entity, Vec3>,
    pub group_center: Vec3,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum ScaleAxis {
    XPos, XNeg,
    YPos, YNeg,
    ZPos, ZNeg,
    Uniform,
}

impl ScaleAxis {
    fn axis(self) -> Axis3d {
        match self {
            ScaleAxis::XPos | ScaleAxis::XNeg => Axis3d::X,
            ScaleAxis::YPos | ScaleAxis::YNeg => Axis3d::Y,
            ScaleAxis::ZPos | ScaleAxis::ZNeg => Axis3d::Z,
            ScaleAxis::Uniform => Axis3d::Y,
        }
    }

    fn sign(self) -> f32 {
        match self {
            ScaleAxis::XPos | ScaleAxis::YPos | ScaleAxis::ZPos | ScaleAxis::Uniform => 1.0,
            ScaleAxis::XNeg | ScaleAxis::YNeg | ScaleAxis::ZNeg => -1.0,
        }
    }

    fn color(self) -> Color {
        match self {
            ScaleAxis::XPos | ScaleAxis::XNeg => Color::srgb(0.95, 0.15, 0.15),
            ScaleAxis::YPos | ScaleAxis::YNeg => Color::srgb(0.15, 0.95, 0.15),
            ScaleAxis::ZPos | ScaleAxis::ZNeg => Color::srgb(0.15, 0.15, 0.95),
            ScaleAxis::Uniform => Color::srgb(1.0, 1.0, 1.0),
        }
    }
}

// ============================================================================
// 2. Plugin Registration
// ============================================================================

pub struct ScaleToolPlugin;

impl Plugin for ScaleToolPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScaleToolState>()
            .add_systems(Update, (
                draw_scale_gizmos,
                handle_scale_interaction,
            ));
    }
}

// ============================================================================
// 3. Gizmo Drawing
// ============================================================================

fn draw_scale_gizmos(
    mut gizmos: Gizmos<TransformGizmoGroup>,
    state: Res<ScaleToolState>,
    query: Query<(&GlobalTransform, Option<&crate::classes::BasePart>), With<SelectionBox>>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
) {
    if !state.active || query.is_empty() { return; }

    let Ok((_, cam_gt, projection)) = cameras.single() else { return };
    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => std::f32::consts::FRAC_PI_4,
    };

    let yellow = Color::srgb(1.0, 1.0, 0.0);

    for (global_transform, base_part) in &query {
        let t = global_transform.compute_transform();
        let pos = t.translation;
        let rot = t.rotation;
        let size = base_part.map(|bp| bp.size).unwrap_or(t.scale);

        // Camera-distance-scaled handle length
        let dist = (pos - cam_gt.translation()).length().max(0.1);
        let scale = dist * (fov * 0.5).tan() * 0.16;
        let handle_len = scale * 0.9;
        let cube_size  = scale * 0.18;

        let local_x = rot * Vec3::X;
        let local_y = rot * Vec3::Y;
        let local_z = rot * Vec3::Z;

        // Handle origins at face centers
        let face_x_pos = pos + local_x * (size.x * 0.5);
        let face_x_neg = pos - local_x * (size.x * 0.5);
        let face_y_pos = pos + local_y * (size.y * 0.5);
        let face_y_neg = pos - local_y * (size.y * 0.5);
        let face_z_pos = pos + local_z * (size.z * 0.5);
        let face_z_neg = pos - local_z * (size.z * 0.5);

        let hl = |ax: ScaleAxis| if state.dragged_axis == Some(ax) { yellow } else { ax.color() };

        // X axis handles
        let x_tip_pos = face_x_pos + local_x * handle_len;
        let x_tip_neg = face_x_neg - local_x * handle_len;
        gizmos.line(face_x_pos, x_tip_pos, hl(ScaleAxis::XPos));
        draw_handle_cube(&mut gizmos, x_tip_pos, rot, cube_size, hl(ScaleAxis::XPos));
        gizmos.line(face_x_neg, x_tip_neg, hl(ScaleAxis::XNeg));
        draw_handle_cube(&mut gizmos, x_tip_neg, rot, cube_size, hl(ScaleAxis::XNeg));

        // Y axis handles
        let y_tip_pos = face_y_pos + local_y * handle_len;
        let y_tip_neg = face_y_neg - local_y * handle_len;
        gizmos.line(face_y_pos, y_tip_pos, hl(ScaleAxis::YPos));
        draw_handle_cube(&mut gizmos, y_tip_pos, rot, cube_size, hl(ScaleAxis::YPos));
        gizmos.line(face_y_neg, y_tip_neg, hl(ScaleAxis::YNeg));
        draw_handle_cube(&mut gizmos, y_tip_neg, rot, cube_size, hl(ScaleAxis::YNeg));

        // Z axis handles
        let z_tip_pos = face_z_pos + local_z * handle_len;
        let z_tip_neg = face_z_neg - local_z * handle_len;
        gizmos.line(face_z_pos, z_tip_pos, hl(ScaleAxis::ZPos));
        draw_handle_cube(&mut gizmos, z_tip_pos, rot, cube_size, hl(ScaleAxis::ZPos));
        gizmos.line(face_z_neg, z_tip_neg, hl(ScaleAxis::ZNeg));
        draw_handle_cube(&mut gizmos, z_tip_neg, rot, cube_size, hl(ScaleAxis::ZNeg));

        // Center uniform-scale cube (white)
        draw_handle_cube(&mut gizmos, pos, rot, cube_size * 1.3,
            if state.dragged_axis == Some(ScaleAxis::Uniform) { yellow }
            else { Color::srgba(1.0, 1.0, 1.0, 0.8) });
    }
}

/// Draw a small wireframe cube at `center` oriented by `rot`.
fn draw_handle_cube(
    gizmos: &mut Gizmos<TransformGizmoGroup>,
    center: Vec3,
    rot: Quat,
    half: f32,
    color: Color,
) {
    let corners = [
        Vec3::new(-half, -half, -half), Vec3::new( half, -half, -half),
        Vec3::new(-half,  half, -half), Vec3::new( half,  half, -half),
        Vec3::new(-half, -half,  half), Vec3::new( half, -half,  half),
        Vec3::new(-half,  half,  half), Vec3::new( half,  half,  half),
    ];
    let wc: Vec<Vec3> = corners.iter().map(|&c| center + rot * c).collect();
    // Bottom
    gizmos.line(wc[0], wc[1], color); gizmos.line(wc[4], wc[5], color);
    gizmos.line(wc[0], wc[4], color); gizmos.line(wc[1], wc[5], color);
    // Top
    gizmos.line(wc[2], wc[3], color); gizmos.line(wc[6], wc[7], color);
    gizmos.line(wc[2], wc[6], color); gizmos.line(wc[3], wc[7], color);
    // Verticals
    gizmos.line(wc[0], wc[2], color); gizmos.line(wc[1], wc[3], color);
    gizmos.line(wc[4], wc[6], color); gizmos.line(wc[5], wc[7], color);
}

// ============================================================================
// 4. Mouse Interaction
// ============================================================================

fn handle_scale_interaction(
    mut state: ResMut<ScaleToolState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
    mut query: Query<(Entity, &GlobalTransform, &mut Transform, Option<&mut crate::classes::BasePart>, Option<&crate::classes::Part>, Option<&mut Mesh3d>, Option<&crate::spawn::MeshSource>), With<SelectionBox>>,
    editor_settings: Res<crate::editor_settings::EditorSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    parent_query: Query<&ChildOf>,
    mut undo_stack: ResMut<crate::undo::UndoStack>,
) {
    if !state.active { return; }

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };
    let Ok((camera, camera_transform, projection)) = cameras.single() else { return };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else { return };

    let camera_forward = camera_transform.forward().as_vec3();
    let camera_right   = camera_transform.right().as_vec3();

    let ctrl_pressed = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);

    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => std::f32::consts::FRAC_PI_4,
    };

    if mouse.just_pressed(MouseButton::Left) {
        let mut clicked_handle = false;

        for (entity, global_transform, transform, basepart_opt, _, _, _) in query.iter() {
            let t = global_transform.compute_transform();
            let pos = t.translation;
            let rot = t.rotation;
            let size = basepart_opt.as_ref().map(|bp| bp.size).unwrap_or(t.scale);

            let dist = (pos - camera_transform.translation()).length().max(0.1);
            let scale = dist * (fov * 0.5).tan() * 0.16;
            let handle_len = scale * 0.9;
            let hit_radius = (scale * 0.22).max(0.05);

            let local_x = rot * Vec3::X;
            let local_y = rot * Vec3::Y;
            let local_z = rot * Vec3::Z;

            let face_x_pos = pos + local_x * (size.x * 0.5);
            let face_x_neg = pos - local_x * (size.x * 0.5);
            let face_y_pos = pos + local_y * (size.y * 0.5);
            let face_y_neg = pos - local_y * (size.y * 0.5);
            let face_z_pos = pos + local_z * (size.z * 0.5);
            let face_z_neg = pos - local_z * (size.z * 0.5);

            let handles: &[(ScaleAxis, Vec3)] = &[
                (ScaleAxis::XPos, face_x_pos + local_x * handle_len),
                (ScaleAxis::XNeg, face_x_neg - local_x * handle_len),
                (ScaleAxis::YPos, face_y_pos + local_y * handle_len),
                (ScaleAxis::YNeg, face_y_neg - local_y * handle_len),
                (ScaleAxis::ZPos, face_z_pos + local_z * handle_len),
                (ScaleAxis::ZNeg, face_z_neg - local_z * handle_len),
                (ScaleAxis::Uniform, pos),
            ];

            let mut best: Option<(ScaleAxis, f32)> = None;
            for &(ax, tip) in handles {
                let d = ray_to_point_distance(ray.origin, *ray.direction, tip);
                if d < hit_radius {
                    if best.is_none() || d < best.unwrap().1 {
                        best = Some((ax, d));
                    }
                }
            }

            if let Some((axis, _)) = best {
                state.dragged_axis = Some(axis);
                state.initial_scale = size;
                state.initial_position = transform.translation;
                state.drag_start_pos = cursor_pos;
                state.dragged_entity = Some(entity);

                state.initial_scales.clear();
                state.initial_positions.clear();
                for (ent, _, trans, bp_opt, _, _, _) in query.iter() {
                    let ent_size = bp_opt.as_ref().map(|bp| bp.size).unwrap_or(Vec3::ONE);
                    state.initial_scales.insert(ent, ent_size);
                    state.initial_positions.insert(ent, trans.translation);
                }

                if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, pos, Vec3::Y) {
                    state.initial_mouse_world = ray.origin + *ray.direction * t;
                }
                clicked_handle = true;
                break;
            }
        }
    } else if mouse.pressed(MouseButton::Left) {
        if let Some(axis) = state.dragged_axis {
            let delta_screen = cursor_pos - state.drag_start_pos;
            let drag_distance = delta_screen.length();
            let base_sensitivity = 0.015;
            let progressive_factor = 1.0 + drag_distance * 0.002;
            let sensitivity = base_sensitivity * progressive_factor;

            let drag_amount = match axis {
                ScaleAxis::YPos | ScaleAxis::YNeg => -delta_screen.y * sensitivity,
                ScaleAxis::XPos | ScaleAxis::XNeg => {
                    let x_dot_right = camera_right.dot(Vec3::X);
                    let x_dot_fwd   = camera_forward.dot(Vec3::X);
                    if x_dot_right.abs() > x_dot_fwd.abs() {
                        delta_screen.x * sensitivity * x_dot_right.signum()
                    } else {
                        -delta_screen.y * sensitivity * x_dot_fwd.signum()
                    }
                }
                ScaleAxis::ZPos | ScaleAxis::ZNeg => {
                    let z_dot_right = camera_right.dot(Vec3::Z);
                    let z_dot_fwd   = camera_forward.dot(Vec3::Z);
                    if z_dot_right.abs() > z_dot_fwd.abs() {
                        delta_screen.x * sensitivity * z_dot_right.signum()
                    } else {
                        -delta_screen.y * sensitivity * z_dot_fwd.signum()
                    }
                }
                ScaleAxis::Uniform => (delta_screen.x - delta_screen.y) * sensitivity * 0.5,
            };

            let direction_mult = match axis {
                ScaleAxis::XNeg | ScaleAxis::YNeg | ScaleAxis::ZNeg => -1.0,
                _ => 1.0,
            };
            let effective_drag = drag_amount * direction_mult;

            let selected_entities: std::collections::HashSet<Entity> = query.iter().map(|(e, ..)| e).collect();

            for (entity, global_transform, mut transform, basepart_opt, part_opt, mesh_opt, mesh_source) in query.iter_mut() {
                if is_descendant(entity, &selected_entities, &parent_query) { continue; }

                if let (Some(initial_size), Some(initial_pos)) = (
                    state.initial_scales.get(&entity),
                    state.initial_positions.get(&entity),
                ) {
                    let new_size = compute_new_size(axis, *initial_size, effective_drag);
                    let final_size = apply_snap(new_size, &editor_settings);
                    let has_mesh_source = mesh_source.is_some();

                    if ctrl_pressed {
                        // Symmetric: position stays centered
                        apply_size_to_entity(
                            &mut transform, basepart_opt, part_opt, mesh_opt,
                            &mut meshes, final_size, *initial_pos, has_mesh_source,
                        );
                    } else {
                        // One-sided: opposite face stays fixed
                        let rot = transform.rotation;
                        let size_diff = final_size - *initial_size;
                        let local_offset = match axis {
                            ScaleAxis::XPos => Vec3::X   * size_diff.x * 0.5,
                            ScaleAxis::XNeg => Vec3::NEG_X * size_diff.x * 0.5,
                            ScaleAxis::YPos => Vec3::Y   * size_diff.y * 0.5,
                            ScaleAxis::YNeg => Vec3::NEG_Y * size_diff.y * 0.5,
                            ScaleAxis::ZPos => Vec3::Z   * size_diff.z * 0.5,
                            ScaleAxis::ZNeg => Vec3::NEG_Z * size_diff.z * 0.5,
                            ScaleAxis::Uniform => Vec3::ZERO,
                        };
                        let world_offset = rot * local_offset;
                        let new_pos = *initial_pos + world_offset;
                        apply_size_to_entity(
                            &mut transform, basepart_opt, part_opt, mesh_opt,
                            &mut meshes, final_size, new_pos, has_mesh_source,
                        );
                    }
                }
            }
        }
    } else if mouse.just_released(MouseButton::Left) {
        if state.dragged_axis.is_some() && !state.initial_scales.is_empty() {
            let mut old_states: Vec<(u64, [f32; 3], [f32; 3])> = Vec::new();
            let mut new_states: Vec<(u64, [f32; 3], [f32; 3])> = Vec::new();

            for (entity, _, transform, basepart_opt, _, _, _) in query.iter() {
                if let (Some(initial_pos), Some(initial_size)) = (
                    state.initial_positions.get(&entity),
                    state.initial_scales.get(&entity),
                ) {
                    let new_size = basepart_opt.as_ref().map(|bp| bp.size).unwrap_or(*initial_size);
                    let pos_changed = (*initial_pos - transform.translation).length() > 0.001;
                    let size_changed = (*initial_size - new_size).length() > 0.001;
                    if pos_changed || size_changed {
                        old_states.push((entity.to_bits(), initial_pos.to_array(), initial_size.to_array()));
                        new_states.push((entity.to_bits(), transform.translation.to_array(), new_size.to_array()));
                    }
                }
            }

            if !old_states.is_empty() {
                undo_stack.push(crate::undo::Action::ScaleEntities { old_states, new_states });
            }
        }

        state.dragged_axis = None;
        state.dragged_entity = None;
        state.initial_scales.clear();
        state.initial_positions.clear();
    }
}

// ============================================================================
// 5. Public Helpers
// ============================================================================

/// Check if a ray hits any scale handle for this entity.
pub fn is_clicking_scale_handle(
    ray: &Ray3d,
    pos: Vec3,
    rot: Quat,
    size: Vec3,
    handle_length: f32,
) -> bool {
    let hit_radius = (handle_length * 0.3).max(0.1);
    let local_x = rot * Vec3::X;
    let local_y = rot * Vec3::Y;
    let local_z = rot * Vec3::Z;

    let tips: &[Vec3] = &[
        pos + local_x * (size.x * 0.5 + handle_length),
        pos - local_x * (size.x * 0.5 + handle_length),
        pos + local_y * (size.y * 0.5 + handle_length),
        pos - local_y * (size.y * 0.5 + handle_length),
        pos + local_z * (size.z * 0.5 + handle_length),
        pos - local_z * (size.z * 0.5 + handle_length),
        pos,
    ];

    tips.iter().any(|&tip| {
        ray_to_point_distance(ray.origin, *ray.direction, tip) < hit_radius
    })
}

// ============================================================================
// Private Helpers
// ============================================================================

fn compute_new_size(axis: ScaleAxis, initial: Vec3, drag: f32) -> Vec3 {
    match axis {
        ScaleAxis::XPos | ScaleAxis::XNeg => Vec3::new((initial.x + drag).max(0.1), initial.y, initial.z),
        ScaleAxis::YPos | ScaleAxis::YNeg => Vec3::new(initial.x, (initial.y + drag).max(0.1), initial.z),
        ScaleAxis::ZPos | ScaleAxis::ZNeg => Vec3::new(initial.x, initial.y, (initial.z + drag).max(0.1)),
        ScaleAxis::Uniform => {
            let f = (1.0 + drag / initial.max_element()).max(0.1);
            initial * f
        }
    }
}

fn apply_snap(size: Vec3, settings: &crate::editor_settings::EditorSettings) -> Vec3 {
    const MIN: f32 = 0.1;
    if settings.snap_enabled {
        let s = settings.snap_size;
        Vec3::new(
            ((size.x / s).round() * s).max(MIN),
            ((size.y / s).round() * s).max(MIN),
            ((size.z / s).round() * s).max(MIN),
        )
    } else {
        size.max(Vec3::splat(MIN))
    }
}

fn apply_size_to_entity(
    transform: &mut Transform,
    basepart_opt: Option<Mut<crate::classes::BasePart>>,
    part_opt: Option<&crate::classes::Part>,
    mesh_opt: Option<Mut<Mesh3d>>,
    meshes: &mut Assets<Mesh>,
    size: Vec3,
    pos: Vec3,
    has_mesh_source: bool,
) {
    transform.translation = pos;

    if let Some(mut bp) = basepart_opt {
        bp.size = size;
        bp.cframe.translation = pos;
    }

    if has_mesh_source {
        // File-system-first: .glb mesh is unit-scale, apply size via Transform.scale
        transform.scale = size;
    } else {
        // Legacy: inline mesh generation at actual size, scale stays ONE
        transform.scale = Vec3::ONE;
        if let (Some(part), Some(mut mesh3d)) = (part_opt, mesh_opt) {
            let new_mesh = match part.shape {
                crate::classes::PartType::Block    => meshes.add(bevy::math::primitives::Cuboid::from_size(size)),
                crate::classes::PartType::Ball     => meshes.add(bevy::math::primitives::Sphere::new(size.x / 2.0)),
                crate::classes::PartType::Cylinder => meshes.add(bevy::math::primitives::Cylinder::new(size.x / 2.0, size.y)),
                _                                  => meshes.add(bevy::math::primitives::Cuboid::from_size(size)),
            };
            mesh3d.0 = new_mesh;
        }
    }
}

fn is_descendant(
    entity: Entity,
    selected_set: &std::collections::HashSet<Entity>,
    parent_query: &Query<&ChildOf>,
) -> bool {
    let mut current = entity;
    while let Ok(child_of) = parent_query.get(current) {
        let parent = child_of.parent();
        if selected_set.contains(&parent) { return true; }
        current = parent;
    }
    false
}
