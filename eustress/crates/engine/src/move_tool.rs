// ============================================================================
// Eustress Engine - Move Tool
// ============================================================================
// ## Table of Contents
// 1. State & types
// 2. Plugin registration
// 3. Tool activation management
// 4. Gizmo drawing (camera-distance-scaled arrows with cones)
// 5. Mouse interaction (axis drag + free drag + surface snapping)
// 6. Public helpers used by part_selection / select_tool
// ============================================================================

#![allow(dead_code)]
#![allow(unused_variables)]

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use crate::selection_box::SelectionBox;
use crate::editor_settings::EditorSettings;
use crate::gizmo_tools::TransformGizmoGroup;
use crate::math_utils::{
    ray_plane_intersection, ray_to_line_segment_distance, calculate_rotated_aabb,
    find_surface_with_physics, find_surface_under_cursor_with_normal,
    calculate_surface_offset, snap_to_grid,
};

// ============================================================================
// 1. State & Types
// ============================================================================

/// Resource tracking the move tool state
#[derive(Resource)]
pub struct MoveToolState {
    pub active: bool,
    /// Which axis handle is being dragged (None = free drag or idle)
    pub dragged_axis: Option<Axis3d>,
    /// Initial world positions of all selected entities at drag start
    pub initial_positions: std::collections::HashMap<Entity, Vec3>,
    /// Initial rotations of all selected entities at drag start
    pub initial_rotations: std::collections::HashMap<Entity, Quat>,
    /// Center of the combined AABB of all selected parts
    pub group_center: Vec3,
    /// World-space mouse position at drag start (for free drag delta)
    pub initial_mouse_world: Vec3,
    /// Screen-space cursor position at drag start
    pub drag_start_pos: Vec2,
    /// True when dragging the part body (not an axis handle)
    pub free_drag: bool,
    /// The entity whose body was clicked to start a free drag
    pub dragged_entity: Option<Entity>,
}

impl Default for MoveToolState {
    fn default() -> Self {
        Self {
            active: false,
            dragged_axis: None,
            initial_positions: std::collections::HashMap::new(),
            initial_rotations: std::collections::HashMap::new(),
            group_center: Vec3::ZERO,
            initial_mouse_world: Vec3::ZERO,
            drag_start_pos: Vec2::ZERO,
            free_drag: false,
            dragged_entity: None,
        }
    }
}

/// World axis enum shared with rotate/scale tools
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Axis3d {
    X,
    Y,
    Z,
}

impl Axis3d {
    pub fn to_vec3(self) -> Vec3 {
        match self {
            Axis3d::X => Vec3::X,
            Axis3d::Y => Vec3::Y,
            Axis3d::Z => Vec3::Z,
        }
    }

    fn color(self) -> Color {
        match self {
            Axis3d::X => Color::srgb(0.95, 0.15, 0.15),
            Axis3d::Y => Color::srgb(0.15, 0.95, 0.15),
            Axis3d::Z => Color::srgb(0.15, 0.15, 0.95),
        }
    }
}

// ============================================================================
// 2. Plugin Registration
// ============================================================================

pub struct MoveToolPlugin;

impl Plugin for MoveToolPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MoveToolState>()
            .add_systems(Update, (
                manage_tool_activation,
                draw_move_gizmos,
                handle_move_interaction,
            ).chain());
    }
}

// ============================================================================
// 3. Tool Activation Management
// ============================================================================

fn manage_tool_activation(
    mut move_state: ResMut<MoveToolState>,
    mut scale_state: ResMut<crate::scale_tool::ScaleToolState>,
    mut rotate_state: ResMut<crate::rotate_tool::RotateToolState>,
    studio_state: Res<crate::ui::StudioState>,
) {
    use crate::ui::Tool;
    move_state.active  = studio_state.current_tool == Tool::Move;
    scale_state.active = studio_state.current_tool == Tool::Scale;
    rotate_state.active = studio_state.current_tool == Tool::Rotate;
}

// ============================================================================
// 4. Gizmo Drawing
// ============================================================================

/// Compute the handle scale so gizmos stay a constant screen size regardless
/// of how far the camera is from the selection.
fn camera_scale_factor(camera_pos: Vec3, target: Vec3, fov_radians: f32) -> f32 {
    let dist = (target - camera_pos).length().max(0.1);
    // Keep handles ~8% of screen height in world units
    dist * (fov_radians * 0.5).tan() * 0.16
}

fn draw_move_gizmos(
    mut gizmos: Gizmos<TransformGizmoGroup>,
    state: Res<MoveToolState>,
    query: Query<(Entity, &GlobalTransform, Option<&crate::classes::BasePart>), With<SelectionBox>>,
    children_query: Query<&Children>,
    child_transforms: Query<(&GlobalTransform, Option<&crate::classes::BasePart>), Without<SelectionBox>>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
) {
    if !state.active || query.is_empty() {
        return;
    }

    // --- Compute group AABB center ---
    let mut bounds_min = Vec3::splat(f32::MAX);
    let mut bounds_max = Vec3::splat(f32::MIN);
    let mut count = 0;

    for (entity, global_transform, base_part) in &query {
        let t = global_transform.compute_transform();
        let size = base_part.map(|bp| bp.size).unwrap_or(t.scale);
        let (mn, mx) = calculate_rotated_aabb(t.translation, size * 0.5, t.rotation);
        bounds_min = bounds_min.min(mn);
        bounds_max = bounds_max.max(mx);
        count += 1;

        if let Ok(children) = children_query.get(entity) {
            for child in children.iter() {
                if let Ok((cg, cbp)) = child_transforms.get(child) {
                    let ct = cg.compute_transform();
                    let cs = cbp.map(|bp| bp.size).unwrap_or(ct.scale);
                    let (cn, cx) = calculate_rotated_aabb(ct.translation, cs * 0.5, ct.rotation);
                    bounds_min = bounds_min.min(cn);
                    bounds_max = bounds_max.max(cx);
                    count += 1;
                }
            }
        }
    }
    if count == 0 { return; }

    let center = (bounds_min + bounds_max) * 0.5;

    // --- Camera-distance-scaled handle length ---
    let Ok((_, cam_gt, projection)) = cameras.single() else { return };
    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => std::f32::consts::FRAC_PI_4,
    };
    let scale = camera_scale_factor(cam_gt.translation(), center, fov);
    let handle_len = scale * 1.0;
    let cone_radius = scale * 0.10;
    let cone_len    = scale * 0.22;

    let yellow = Color::srgb(1.0, 1.0, 0.0);

    for axis in [Axis3d::X, Axis3d::Y, Axis3d::Z] {
        let dir = axis.to_vec3();
        let highlighted = state.dragged_axis == Some(axis);
        let color = if highlighted { yellow } else { axis.color() };

        // Positive direction
        let tip_pos = center + dir * handle_len;
        gizmos.line(center, tip_pos, color);
        draw_cone(&mut gizmos, tip_pos, dir, cone_radius, cone_len, color);

        // Negative direction
        let tip_neg = center - dir * handle_len;
        gizmos.line(center, tip_neg, color);
        draw_cone(&mut gizmos, tip_neg, -dir, cone_radius, cone_len, color);
    }

    // Small center sphere
    gizmos.sphere(
        Isometry3d::from_translation(center),
        scale * 0.08,
        Color::srgba(1.0, 1.0, 1.0, 0.8),
    );
}

/// Draw an arrow-head cone at `tip` pointing in `dir`.
fn draw_cone(
    gizmos: &mut Gizmos<TransformGizmoGroup>,
    tip: Vec3,
    dir: Vec3,
    radius: f32,
    length: f32,
    color: Color,
) {
    let base = tip - dir * length;
    let up = if dir.abs().dot(Vec3::Y) > 0.9 { Vec3::X } else { Vec3::Y };
    let right   = dir.cross(up).normalize() * radius;
    let forward = dir.cross(right.normalize()).normalize() * radius;

    const SEGS: usize = 8;
    for i in 0..SEGS {
        let a0 = (i as f32 / SEGS as f32) * std::f32::consts::TAU;
        let a1 = ((i + 1) as f32 / SEGS as f32) * std::f32::consts::TAU;
        let p0 = base + right * a0.cos() + forward * a0.sin();
        let p1 = base + right * a1.cos() + forward * a1.sin();
        gizmos.line(tip, p0, color);
        gizmos.line(p0, p1, color);
    }
}

// ============================================================================
// 5. Mouse Interaction
// ============================================================================

fn handle_move_interaction(
    mut state: ResMut<MoveToolState>,
    settings: Res<EditorSettings>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
    mut query: Query<(Entity, &GlobalTransform, &mut Transform, Option<&mut crate::classes::BasePart>), With<SelectionBox>>,
    children_query: Query<&Children>,
    child_global_transforms: Query<(&GlobalTransform, Option<&crate::classes::BasePart>), Without<SelectionBox>>,
    unselected_query: Query<(Entity, &GlobalTransform, &Mesh3d, Option<&crate::rendering::PartEntity>, Option<&crate::classes::Instance>, Option<&crate::classes::BasePart>), Without<SelectionBox>>,
    parent_query: Query<&ChildOf>,
    mut undo_stack: ResMut<crate::undo::UndoStack>,
    spatial_query: SpatialQuery,
) {
    if !state.active { return; }

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };
    let Ok((camera, camera_transform, projection)) = cameras.single() else { return };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else { return };
    let camera_forward = camera_transform.forward().as_vec3();

    // --- Compute group bounds (needed for handle detection) ---
    let (center, avg_size, handle_len) = {
        let mut bmin = Vec3::splat(f32::MAX);
        let mut bmax = Vec3::splat(f32::MIN);
        let mut cnt = 0;
        for (entity, gt, _, bp) in query.iter() {
            let t = gt.compute_transform();
            let s = bp.as_ref().map(|b| b.size).unwrap_or(t.scale);
            let (mn, mx) = calculate_rotated_aabb(t.translation, s * 0.5, t.rotation);
            bmin = bmin.min(mn); bmax = bmax.max(mx); cnt += 1;
            if let Ok(children) = children_query.get(entity) {
                for child in children.iter() {
                    if let Ok((cg, cbp)) = child_global_transforms.get(child) {
                        let ct = cg.compute_transform();
                        let cs = cbp.map(|b| b.size).unwrap_or(ct.scale);
                        let (cn, cx) = calculate_rotated_aabb(ct.translation, cs * 0.5, ct.rotation);
                        bmin = bmin.min(cn); bmax = bmax.max(cx); cnt += 1;
                    }
                }
            }
        }
        if cnt == 0 { return; }
        let c = (bmin + bmax) * 0.5;
        let fov = match projection {
            Projection::Perspective(p) => p.fov,
            _ => std::f32::consts::FRAC_PI_4,
        };
        let scale = camera_scale_factor(camera_transform.translation(), c, fov);
        (c, (bmax - bmin).max_element(), scale * 1.0)
    };

    // ---- Mouse Down ----
    if mouse.just_pressed(MouseButton::Left) {
        if query.is_empty() { return; }

        // 1. Check axis handle click
        if let Some(axis) = detect_axis_hit(&ray, center, handle_len, camera_transform) {
            state.dragged_axis = Some(axis);
            state.free_drag = false;
            state.group_center = center;
            state.drag_start_pos = cursor_pos;

            // Store initial state for all selected parts
            state.initial_positions.clear();
            state.initial_rotations.clear();
            for (entity, _, transform, _) in query.iter() {
                state.initial_positions.insert(entity, transform.translation);
                state.initial_rotations.insert(entity, transform.rotation);
            }

            // Compute initial mouse world position on the axis drag plane
            let axis_vec = axis.to_vec3();
            let plane_normal = get_axis_drag_plane_normal(axis_vec, camera_forward);
            if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, center, plane_normal) {
                state.initial_mouse_world = ray.origin + *ray.direction * t;
            }
            return;
        }

        // 2. Check if clicking on a selected part body → free drag
        let selected_entities: Vec<Entity> = query.iter().map(|(e, ..)| e).collect();
        for (entity, gt, _, bp) in query.iter() {
            let t = gt.compute_transform();
            let size = bp.as_ref().map(|b| b.size).unwrap_or(t.scale);
            if crate::math_utils::ray_intersects_part_rotated(&ray, t.translation, t.rotation, size) {
                state.free_drag = true;
                state.dragged_axis = None;
                state.dragged_entity = Some(entity);
                state.group_center = center;
                state.drag_start_pos = cursor_pos;

                state.initial_positions.clear();
                state.initial_rotations.clear();
                for (ent, _, transform, _) in query.iter() {
                    state.initial_positions.insert(ent, transform.translation);
                    state.initial_rotations.insert(ent, transform.rotation);
                }

                // Initial mouse world on horizontal plane at group center height
                if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, center, Vec3::Y) {
                    state.initial_mouse_world = ray.origin + *ray.direction * t;
                }
                return;
            }
        }
    }

    // ---- Mouse Held ----
    else if mouse.pressed(MouseButton::Left) {
        if let Some(axis) = state.dragged_axis {
            // Axis-constrained drag
            let axis_vec = axis.to_vec3();
            let plane_normal = get_axis_drag_plane_normal(axis_vec, camera_forward);

            if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, state.group_center, plane_normal) {
                let current_world = ray.origin + *ray.direction * t;
                let raw_delta = current_world - state.initial_mouse_world;

                // Project delta onto the axis
                let axis_delta = raw_delta.dot(axis_vec);
                let snapped_delta = if settings.snap_enabled {
                    (axis_delta / settings.snap_size).round() * settings.snap_size
                } else {
                    axis_delta
                };

                let selected_set: std::collections::HashSet<Entity> = query.iter().map(|(e, ..)| e).collect();

                for (entity, _, mut transform, base_part_opt) in query.iter_mut() {
                    if is_descendant(entity, &selected_set, &parent_query) { continue; }
                    if let Some(initial_pos) = state.initial_positions.get(&entity) {
                        let new_pos = *initial_pos + axis_vec * snapped_delta;
                        transform.translation = new_pos;
                        if let Some(mut bp) = base_part_opt {
                            bp.cframe.translation = new_pos;
                        }
                    }
                }
            }
        } else if state.free_drag {
            // Free drag — surface snapping (same as select tool)
            let selected_entities: Vec<Entity> = query.iter().map(|(e, ..)| e).collect();

            let surface_hit = find_surface_with_physics(&spatial_query, &ray, &selected_entities)
                .map(|(pt, norm, ent)| (pt, norm, Some(ent)))
                .or_else(|| {
                    find_surface_under_cursor_with_normal(&ray, &unselected_query, &selected_entities)
                        .map(|(pt, norm)| (pt, norm, None))
                });

            let dragged_entity = state.dragged_entity;
            let leader_size = dragged_entity
                .and_then(|e| query.get(e).ok())
                .and_then(|(_, _, _, bp)| bp.as_ref().map(|b| b.size))
                .unwrap_or(Vec3::ONE);
            let leader_rot = dragged_entity
                .and_then(|e| state.initial_rotations.get(&e).copied())
                .unwrap_or(Quat::IDENTITY);
            let leader_initial = dragged_entity
                .and_then(|e| state.initial_positions.get(&e).copied())
                .unwrap_or(state.group_center);

            let target_pos = if let Some((hit_point, hit_normal, _)) = surface_hit {
                let offset = calculate_surface_offset(&leader_size, &leader_rot, &hit_normal);
                hit_point + hit_normal * (offset + 0.005)
            } else {
                // Fallback: drag on horizontal plane
                if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, state.group_center, Vec3::Y) {
                    let ground = ray.origin + *ray.direction * t;
                    let offset = calculate_surface_offset(&leader_size, &leader_rot, &Vec3::Y);
                    Vec3::new(ground.x, offset + 0.005, ground.z)
                } else {
                    leader_initial
                }
            };

            let final_target = if settings.snap_enabled {
                snap_to_grid(target_pos, settings.snap_size)
            } else {
                target_pos
            };

            let selected_set: std::collections::HashSet<Entity> = query.iter().map(|(e, ..)| e).collect();
            let pivot = leader_initial;

            for (entity, _, mut transform, base_part_opt) in query.iter_mut() {
                if is_descendant(entity, &selected_set, &parent_query) { continue; }
                if let Some(initial_pos) = state.initial_positions.get(&entity) {
                    let rel = *initial_pos - pivot;
                    let new_pos = final_target + rel;
                    transform.translation = new_pos;
                    if let Some(mut bp) = base_part_opt {
                        bp.cframe.translation = new_pos;
                    }
                }
            }
        }
    }

    // ---- Mouse Released ----
    else if mouse.just_released(MouseButton::Left) {
        if (state.dragged_axis.is_some() || state.free_drag) && !state.initial_positions.is_empty() {
            let mut old_transforms = Vec::new();
            let mut new_transforms = Vec::new();

            for (entity, _, transform, _) in query.iter() {
                if let Some(initial_pos) = state.initial_positions.get(&entity) {
                    if let Some(initial_rot) = state.initial_rotations.get(&entity) {
                        if (*initial_pos - transform.translation).length() > 0.001 {
                            old_transforms.push((entity.to_bits(), initial_pos.to_array(), initial_rot.to_array()));
                            new_transforms.push((entity.to_bits(), transform.translation.to_array(), transform.rotation.to_array()));
                        }
                    }
                }
            }

            if !old_transforms.is_empty() {
                undo_stack.push(crate::undo::Action::TransformEntities { old_transforms, new_transforms });
            }
        }

        state.dragged_axis = None;
        state.free_drag = false;
        state.dragged_entity = None;
        state.initial_positions.clear();
        state.initial_rotations.clear();
    }
}

// ============================================================================
// 6. Public Helpers
// ============================================================================

/// Returns the best-matching axis handle hit by the ray, or None.
/// Uses the real ray-to-segment distance with the fixed math_utils implementation.
pub fn detect_axis_hit(
    ray: &Ray3d,
    center: Vec3,
    handle_len: f32,
    camera_transform: &GlobalTransform,
) -> Option<Axis3d> {
    // Hit radius scales with handle length so small and large handles are equally clickable
    let hit_radius = (handle_len * 0.18).clamp(0.05, 0.6);

    let mut best: Option<(Axis3d, f32)> = None;

    for axis in [Axis3d::X, Axis3d::Y, Axis3d::Z] {
        let dir = axis.to_vec3();
        for sign in [1.0_f32, -1.0] {
            let seg_end = center + dir * handle_len * sign;
            let dist = ray_to_line_segment_distance(ray.origin, *ray.direction, center, seg_end);
            if dist < hit_radius {
                // Depth-sort: prefer the handle closest to the camera
                let mid = (center + seg_end) * 0.5;
                let cam_dist = (mid - camera_transform.translation()).length();
                if best.is_none() || cam_dist < best.unwrap().1 {
                    best = Some((axis, cam_dist));
                }
            }
        }
    }

    best.map(|(a, _)| a)
}

/// Public wrapper used by part_selection and select_tool to avoid interfering with move handles.
pub fn is_clicking_move_handle(
    ray: &Ray3d,
    center: Vec3,
    _size: Vec3,
    handle_len: f32,
    camera_transform: &GlobalTransform,
) -> bool {
    detect_axis_hit(ray, center, handle_len, camera_transform).is_some()
}

// ============================================================================
// Private Helpers
// ============================================================================

/// Returns the best drag-plane normal for an axis: the plane that contains
/// the axis and is most face-on to the camera.
fn get_axis_drag_plane_normal(axis: Vec3, camera_forward: Vec3) -> Vec3 {
    let perp = axis.cross(camera_forward);
    if perp.length_squared() < 0.001 {
        return camera_forward;
    }
    axis.cross(perp).normalize()
}

/// Returns true if `entity` has any ancestor that is in `selected_set`.
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
