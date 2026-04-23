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
use crate::selection_box::Selected;
use crate::gizmo_tools::TransformGizmoGroup;
use crate::math_utils::{ray_plane_intersection, ray_to_point_distance};
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
    /// Gizmo rotation captured at drag start. `IDENTITY` for World mode,
    /// active entity's rotation for Local. Unused in current drag math
    /// (which is camera-relative) but preserved for future numeric-input
    /// + parametric-scale features where the frame matters.
    pub drag_rotation: Quat,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum ScaleAxis {
    XPos, XNeg,
    YPos, YNeg,
    ZPos, ZNeg,
    Uniform,
}

impl ScaleAxis {
    pub fn axis(self) -> Axis3d {
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

/// Absolute-resize event — fire this to set a part's `BasePart.size`
/// to an exact vector. The handler reuses the same `apply_size_to_entity`
/// path the scale-gizmo + numeric-input commit use, so primitive-mesh
/// regeneration, custom-GLB `Transform.scale` mode, and `BasePart.cframe`
/// bookkeeping all stay consistent.
///
/// Emitted by:
/// * The Properties-panel `"Size"` copy/paste write-back — without
///   this, pasting a size would update `Transform.scale` only (which
///   parts keep at `[1, 1, 1]`) and leave the visible dimensions
///   unchanged.
/// * Any future tool that wants to resize a part by an absolute
///   value (e.g. a paste-props MCP action).
#[derive(bevy::prelude::Message, Debug, Clone)]
pub struct ResizePartEvent {
    pub entity: Entity,
    pub new_size: Vec3,
}

impl Plugin for ScaleToolPlugin {
    fn build(&self, app: &mut App) {
        // Gizmo drawing moved to `scale_handles::ScaleHandlesPlugin`
        // (mesh-based, renders through the same pipeline as the
        // selection wireframe instead of Bevy's gizmo graph).
        app.init_resource::<ScaleToolState>()
            .add_message::<ResizePartEvent>()
            .add_systems(Update, (
                handle_scale_interaction,
                // Numeric-input commit — applies typed size exactly and
                // finalizes the drag. Runs after cursor-driven drag so
                // Enter wins over any in-progress cursor delta.
                finalize_numeric_input_on_scale.after(handle_scale_interaction),
                // Absolute-resize handler for `ResizePartEvent` (emitted
                // by the Properties-panel paste path + future tools).
                handle_resize_part_events,
            ));
    }
}

/// Drain `ResizePartEvent`s emitted by the Properties-panel paste
/// handler (etc.) and apply each via the shared `apply_size_to_entity`
/// helper so primitive mesh regen + transform adjustment stay in one
/// code path with the scale-gizmo commit.
fn handle_resize_part_events(
    mut events: MessageReader<ResizePartEvent>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut entities: Query<(
        &mut Transform,
        Option<&mut crate::classes::BasePart>,
        Option<&crate::classes::Part>,
        Option<&mut Mesh3d>,
        Option<&crate::spawn::MeshSource>,
    )>,
) {
    for event in events.read() {
        let Ok((mut transform, base_part, part, mesh, mesh_source)) = entities.get_mut(event.entity)
        else { continue };
        let has_mesh_source = mesh_source.is_some();
        let pos = transform.translation;
        apply_size_to_entity(
            &mut *transform,
            base_part,
            part,
            mesh,
            &mut meshes,
            event.new_size,
            pos,
            has_mesh_source,
        );
    }
}

// ============================================================================
// 3. Gizmo Drawing
// ============================================================================

fn draw_scale_gizmos(
    mut gizmos: Gizmos<TransformGizmoGroup>,
    state: Res<ScaleToolState>,
    query: Query<(&GlobalTransform, Option<&crate::classes::BasePart>), With<Selected>>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
) {
    if !state.active || query.is_empty() { return; }

    let Some((_, cam_gt, projection)) = cameras.iter().find(|(c, _, _)| c.order == 0) else { return };
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
    mut query: Query<(Entity, &GlobalTransform, &mut Transform, Option<&mut crate::classes::BasePart>, Option<&crate::classes::Part>, Option<&mut Mesh3d>, Option<&crate::spawn::MeshSource>), With<Selected>>,
    editor_settings: Res<crate::editor_settings::EditorSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    parent_query: Query<&ChildOf>,
    mut undo_stack: ResMut<crate::undo::UndoStack>,
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
    studio_state: Option<Res<crate::ui::StudioState>>,
) {
    if !state.active { return; }

    // Transform mode governs whether scale handles are axis-aligned to
    // world (World mode) or rotated to match the active entity (Local).
    // Hit test must use the same rotation as `scale_handles::sync_scale_handle_root`
    // renders — otherwise clicking the rotated Local-mode cube misses.
    let transform_mode = studio_state
        .as_ref()
        .map(|s| s.transform_mode)
        .unwrap_or(crate::ui::TransformMode::World);

    // Escape cancels an in-progress scale and restores pre-drag sizes.
    if keys.just_pressed(KeyCode::Escape) {
        if state.dragged_axis.is_some() {
            for (entity, _, mut transform, basepart_opt, _, _, _) in query.iter_mut() {
                if let Some(initial_size) = state.initial_scales.get(&entity).copied() {
                    if let Some(mut bp) = basepart_opt {
                        bp.size = initial_size;
                    }
                }
                if let Some(initial_pos) = state.initial_positions.get(&entity).copied() {
                    transform.translation = initial_pos;
                }
            }
            state.dragged_axis = None;
            state.dragged_entity = None;
            state.initial_scales.clear();
            state.initial_positions.clear();
            return;
        }
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };
    
    // Block NEW drags when cursor is over UI panels (outside 3D viewport).
    // Allow in-progress drags to continue even if cursor leaves the viewport.
    // ViewportBounds is physical px, cursor_pos is logical — go through
    // contains_logical so DPI-scaled displays don't reject every click.
    if state.dragged_axis.is_none() {
        if let Some(vb) = viewport_bounds.as_deref() {
            let scale = window.scale_factor() as f32;
            if !vb.contains_logical(cursor_pos, scale) { return; }
        }
    }
    let Some((camera, camera_transform, projection)) = cameras.iter().find(|(c, _, _)| c.order == 0) else { return };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else { return };

    let camera_forward = camera_transform.forward().as_vec3();
    let camera_right   = camera_transform.right().as_vec3();

    let ctrl_pressed = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);

    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => std::f32::consts::FRAC_PI_4,
    };

    if mouse.just_pressed(MouseButton::Left) {
        // Group-aware hit test: positions match `scale_handles::sync_scale_handle_root`
        // exactly. We read the shared constants from scale_handles so
        // visual and hit zones can never drift apart.
        use crate::scale_handles::{
            HANDLE_EXT_FRAC, CUBE_SIZE_FRAC, CENTER_CUBE_FRAC, MIN_SCREEN_FRACTION,
        };

        // Compute group frame (center + extent + active rotation).
        let mut bounds_min = Vec3::splat(f32::MAX);
        let mut bounds_max = Vec3::splat(f32::MIN);
        let mut active_rotation = Quat::IDENTITY;
        let mut entity_count = 0;
        for (_entity, global_transform, _trans, basepart_opt, _, _, _) in query.iter() {
            let t = global_transform.compute_transform();
            let size = basepart_opt.as_ref().map(|bp| bp.size).unwrap_or(t.scale);
            let (mn, mx) = crate::math_utils::calculate_rotated_aabb(t.translation, size * 0.5, t.rotation);
            bounds_min = bounds_min.min(mn);
            bounds_max = bounds_max.max(mx);
            active_rotation = t.rotation;
            entity_count += 1;
        }
        if entity_count == 0 { return; }

        let group_center = (bounds_min + bounds_max) * 0.5;
        let group_extent = (bounds_max - bounds_min) * 0.5;
        let size_extent = group_extent.max_element();

        // Match scale_handles' min-size floor so tiny parts still have
        // clickable handles.
        let cam_dist = (group_center - camera_transform.translation()).length().max(0.1);
        let min_screen_size = cam_dist * (fov * 0.5).tan() * MIN_SCREEN_FRACTION;
        let effective_extent = size_extent.max(min_screen_size);

        // Layout constants from scale_handles.rs — shared so visual and
        // hit zones stay in lockstep.
        let handle_ext = effective_extent * HANDLE_EXT_FRAC;
        let cube_size = effective_extent * CUBE_SIZE_FRAC;
        let center_size = effective_extent * CENTER_CUBE_FRAC;
        // Hit radius: 75% of the cube's half-extent, so user can be
        // slightly off the cube and still register.
        let hit_radius = (cube_size * 0.75).max(0.05);
        let center_hit_radius = (center_size * 0.75).max(0.05);

        // Gizmo rotation captured for this click — matches scale_handles'
        // rotation choice. Locked into drag_rotation for the whole drag.
        let rotation = crate::move_tool::gizmo_rotation_for(
            transform_mode,
            query.iter().map(|(_, gt, _, _, _, _, _)| gt.compute_transform().rotation),
        );

        // Candidate handle positions in world-space, matching
        // scale_handles.rs layout: face cubes at `extent + handle_ext`
        // along each ±-axis direction; center cube at group origin.
        let dirs: &[(ScaleAxis, Vec3)] = &[
            (ScaleAxis::XPos, Vec3::X),
            (ScaleAxis::XNeg, Vec3::NEG_X),
            (ScaleAxis::YPos, Vec3::Y),
            (ScaleAxis::YNeg, Vec3::NEG_Y),
            (ScaleAxis::ZPos, Vec3::Z),
            (ScaleAxis::ZNeg, Vec3::NEG_Z),
        ];

        let mut best: Option<(ScaleAxis, f32)> = None;
        // Face cubes
        for &(ax, dir) in dirs {
            let world_pos = group_center + rotation * dir * (effective_extent + handle_ext);
            let d = ray_to_point_distance(ray.origin, *ray.direction, world_pos);
            if d < hit_radius {
                if best.map_or(true, |(_, dist)| d < dist) {
                    best = Some((ax, d));
                }
            }
        }
        // Center cube (uniform)
        {
            let d = ray_to_point_distance(ray.origin, *ray.direction, group_center);
            if d < center_hit_radius {
                if best.map_or(true, |(_, dist)| d < dist) {
                    best = Some((ScaleAxis::Uniform, d));
                }
            }
        }

        if let Some((axis, _)) = best {
            state.dragged_axis = Some(axis);
            // Store drag rotation for consistency with dragged delta math.
            // Unused inside scale_tool today (drag is camera-relative), but
            // future extensions (numeric input, undo label) may need it.
            state.drag_rotation = rotation;
            state.drag_start_pos = cursor_pos;
            state.initial_mouse_world = ray.origin;

            state.initial_scales.clear();
            state.initial_positions.clear();
            for (ent, _, trans, bp_opt, _, _, _) in query.iter() {
                let ent_size = bp_opt.as_ref().map(|bp| bp.size).unwrap_or(Vec3::ONE);
                state.initial_scales.insert(ent, ent_size);
                state.initial_positions.insert(ent, trans.translation);
            }
            state.group_center = group_center;

            if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, group_center, Vec3::Y) {
                state.initial_mouse_world = ray.origin + *ray.direction * t;
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
                    // Phase-2 Scale Lock — treat face drags as uniform
                    // when the setting is on. Preserves axis ratios.
                    let effective_axis = if editor_settings.scale_lock_proportional
                        && axis != ScaleAxis::Uniform
                    {
                        ScaleAxis::Uniform
                    } else {
                        axis
                    };
                    let new_size = compute_new_size(effective_axis, *initial_size, effective_drag);
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

/// Apply a `NumericInputCommittedEvent` to an in-progress Scale drag.
///
/// Semantics of the typed value:
/// - **Uniform** axis (center handle): `value` is a uniform multiplier
///   (relative = `initial * (1 + value)`; absolute = `initial * value`).
/// - **Per-axis face handle** (XPos/XNeg/YPos/YNeg/ZPos/ZNeg): `value`
///   is the absolute target size along that axis (relative = delta).
///   One-sided scale keeps the opposite face fixed — same as the
///   cursor-driven drag code path.
///
/// Matches the existing mouse-release in that it pushes a `ScaleEntities`
/// undo action; TOML persistence for Scale is handled elsewhere and is
/// not duplicated here.
#[allow(clippy::too_many_arguments)]
fn finalize_numeric_input_on_scale(
    mut committed: MessageReader<crate::numeric_input::NumericInputCommittedEvent>,
    mut state: ResMut<ScaleToolState>,
    mut query: Query<(
        Entity,
        &mut Transform,
        Option<&mut crate::classes::BasePart>,
    ), With<Selected>>,
    parent_query: Query<&ChildOf>,
    mut undo_stack: ResMut<crate::undo::UndoStack>,
) {
    use crate::numeric_input::NumericInputOwner;

    for event in committed.read() {
        if event.owner != NumericInputOwner::Scale { continue; }
        let Some(axis) = state.dragged_axis else { continue; };
        if state.initial_scales.is_empty() { continue; }

        let value = event.value;
        let relative = event.relative;

        let selected_set: std::collections::HashSet<Entity> = query.iter().map(|(e, ..)| e).collect();

        let mut old_states: Vec<(u64, [f32; 3], [f32; 3])> = Vec::new();
        let mut new_states: Vec<(u64, [f32; 3], [f32; 3])> = Vec::new();

        for (entity, mut transform, basepart_opt) in query.iter_mut() {
            if is_descendant(entity, &selected_set, &parent_query) { continue; }
            let Some(initial_size) = state.initial_scales.get(&entity).copied() else { continue };
            let Some(initial_pos)  = state.initial_positions.get(&entity).copied() else { continue };

            let new_size = match axis {
                ScaleAxis::Uniform => {
                    let factor = if relative { 1.0 + value } else { value };
                    (initial_size * factor).max(Vec3::splat(0.1))
                }
                _ => {
                    // Per-axis absolute-or-relative. Component picked
                    // by the axis; other two stay.
                    let component = match axis {
                        ScaleAxis::XPos | ScaleAxis::XNeg => initial_size.x,
                        ScaleAxis::YPos | ScaleAxis::YNeg => initial_size.y,
                        ScaleAxis::ZPos | ScaleAxis::ZNeg => initial_size.z,
                        ScaleAxis::Uniform => unreachable!(),
                    };
                    let target = if relative { component + value } else { value };
                    let target = target.max(0.1);
                    match axis {
                        ScaleAxis::XPos | ScaleAxis::XNeg => Vec3::new(target, initial_size.y, initial_size.z),
                        ScaleAxis::YPos | ScaleAxis::YNeg => Vec3::new(initial_size.x, target, initial_size.z),
                        ScaleAxis::ZPos | ScaleAxis::ZNeg => Vec3::new(initial_size.x, initial_size.y, target),
                        ScaleAxis::Uniform => unreachable!(),
                    }
                }
            };

            // One-sided scale — opposite face stays fixed. Matches the
            // cursor drag when Ctrl isn't held (Ctrl = symmetric, which
            // keeps the origin centered; skipping that v1).
            let rot = transform.rotation;
            let size_diff = new_size - initial_size;
            let local_offset = match axis {
                ScaleAxis::XPos    => Vec3::X     * size_diff.x * 0.5,
                ScaleAxis::XNeg    => Vec3::NEG_X * size_diff.x * 0.5,
                ScaleAxis::YPos    => Vec3::Y     * size_diff.y * 0.5,
                ScaleAxis::YNeg    => Vec3::NEG_Y * size_diff.y * 0.5,
                ScaleAxis::ZPos    => Vec3::Z     * size_diff.z * 0.5,
                ScaleAxis::ZNeg    => Vec3::NEG_Z * size_diff.z * 0.5,
                ScaleAxis::Uniform => Vec3::ZERO,
            };
            let world_offset = rot * local_offset;
            let new_pos = initial_pos + world_offset;

            // Record undo BEFORE we mutate.
            let size_changed = (initial_size - new_size).length() > 0.001;
            let pos_changed  = (initial_pos - new_pos).length() > 0.001;
            if size_changed || pos_changed {
                old_states.push((entity.to_bits(), initial_pos.to_array(), initial_size.to_array()));
                new_states.push((entity.to_bits(), new_pos.to_array(),     new_size.to_array()));
            }

            // Apply — file-system-first parts are unit-scale GLBs so
            // size lands on Transform.scale. BasePart.size + cframe
            // stay the authoritative source the TOML writer reads.
            transform.translation = new_pos;
            transform.scale = new_size;
            if let Some(mut bp) = basepart_opt {
                bp.size = new_size;
                bp.cframe.translation = new_pos;
            }
        }

        if !old_states.is_empty() {
            undo_stack.push(crate::undo::Action::ScaleEntities { old_states, new_states });
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

/// Check if a ray hits any scale handle for the GROUP bounds.
/// Layout matches `scale_handles::sync_scale_handle_root` — face cubes
/// at `group_extent + handle_ext` along each axis (rotated by `rotation`),
/// plus the uniform-scale cube at the group center.
///
/// `group_extent` is the half-size of the group's AABB
/// (`group_bounds_max - group_center`).
/// `effective_extent` should be `group_extent.max_element()` clamped to
/// `MIN_SCREEN_FRACTION * camera_distance` so tiny parts still have
/// clickable handles — matches the same clamp in `scale_handles.rs`.
///
/// `rotation`: `Quat::IDENTITY` for World mode, active entity's rotation
/// for Local mode. Must match the rotation the visual layout uses.
pub fn is_clicking_scale_handle_group(
    ray: &Ray3d,
    group_center: Vec3,
    effective_extent: f32,
    rotation: Quat,
) -> bool {
    use crate::scale_handles::{HANDLE_EXT_FRAC, CUBE_SIZE_FRAC, CENTER_CUBE_FRAC};

    let handle_ext = effective_extent * HANDLE_EXT_FRAC;
    let cube_size = effective_extent * CUBE_SIZE_FRAC;
    let center_size = effective_extent * CENTER_CUBE_FRAC;
    let hit_radius = (cube_size * 0.75).max(0.05);
    let center_hit_radius = (center_size * 0.75).max(0.05);

    let dirs: [Vec3; 6] = [
        Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z,
    ];

    // Face cubes
    for dir in dirs {
        let world_pos = group_center + rotation * dir * (effective_extent + handle_ext);
        if ray_to_point_distance(ray.origin, *ray.direction, world_pos) < hit_radius {
            return true;
        }
    }
    // Uniform center cube
    if ray_to_point_distance(ray.origin, *ray.direction, group_center) < center_hit_radius {
        return true;
    }
    false
}

/// Convenience: compute effective extent (with min-screen floor) for a
/// group, matching `scale_handles::sync_scale_handle_root`. Use this when
/// calling `is_clicking_scale_handle_group` to stay in lockstep.
pub fn effective_scale_extent(
    group_extent: Vec3,
    group_center: Vec3,
    camera_translation: Vec3,
    fov: f32,
) -> f32 {
    use crate::scale_handles::MIN_SCREEN_FRACTION;
    let cam_dist = (group_center - camera_translation).length().max(0.1);
    let min_screen_size = cam_dist * (fov * 0.5).tan() * MIN_SCREEN_FRACTION;
    group_extent.max_element().max(min_screen_size)
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
