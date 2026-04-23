// ============================================================================
// Eustress Engine - Rotate Tool
// ============================================================================
// ## Table of Contents
// 1. State & types
// 2. Plugin registration
// 3. Gizmo drawing (camera-scaled arc rings at group center)
// 4. Mouse interaction (arc drag, angle snapping)
// 5. Public helpers
// ============================================================================

#![allow(dead_code)]
#![allow(unused_variables)]

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::selection_box::Selected;
use crate::gizmo_tools::TransformGizmoGroup;
use crate::math_utils::{ray_plane_intersection, calculate_rotated_aabb};
use crate::move_tool::Axis3d;

// ============================================================================
// 1. State & Types
// ============================================================================

#[derive(Resource, Default)]
pub struct RotateToolState {
    pub active: bool,
    /// Which ring axis is being dragged
    pub dragged_axis: Option<Axis3d>,
    /// Angle (radians) at drag start
    pub drag_start_angle: f32,
    /// Screen-space cursor at drag start
    pub drag_start_pos: Vec2,
    /// Initial rotation of the primary entity
    pub initial_rotation: Quat,
    /// Initial rotations of ALL selected entities
    pub initial_rotations: std::collections::HashMap<Entity, Quat>,
    /// Initial positions of ALL selected entities (for pivot rotation)
    pub initial_positions: std::collections::HashMap<Entity, Vec3>,
    /// Group center at drag start (pivot point)
    pub group_center: Vec3,
    /// Gizmo rotation captured at drag start. World mode → IDENTITY;
    /// Local mode → active entity's rotation at press time. Kept stable
    /// for the entire drag so Local-mode rotation doesn't feedback-loop
    /// as the entity rotates from its own motion.
    pub drag_rotation: Quat,
}

// ============================================================================
// 2. Plugin Registration
// ============================================================================

pub struct RotateToolPlugin;

impl Plugin for RotateToolPlugin {
    fn build(&self, app: &mut App) {
        // Gizmo drawing moved to `rotate_handles::RotateHandlesPlugin`.
        app.init_resource::<RotateToolState>()
            .add_systems(Update, (
                handle_rotate_interaction,
                // Numeric-input commit — applies typed angle exactly and
                // finalizes the drag. Runs after cursor-driven drag so
                // Enter wins over any in-progress cursor delta.
                finalize_numeric_input_on_rotate.after(handle_rotate_interaction),
            ));
    }
}

// ============================================================================
// 3. Gizmo Drawing
// ============================================================================

fn draw_rotate_gizmos(
    mut gizmos: Gizmos<TransformGizmoGroup>,
    state: Res<RotateToolState>,
    query: Query<(Entity, &GlobalTransform, Option<&crate::classes::BasePart>), With<Selected>>,
    children_query: Query<&Children>,
    child_transforms: Query<(&GlobalTransform, Option<&crate::classes::BasePart>), Without<Selected>>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
) {
    if !state.active || query.is_empty() { return; }

    // Compute group bounding box and center
    let (center, bbox_extent) = compute_group_center_and_extent(&query, &children_query, &child_transforms);

    // Camera-distance-scaled radius, incorporating object bounding extent
    let Some((_, cam_gt, projection)) = cameras.iter().find(|(c, _, _)| c.order == 0) else { return };
    let radius = compute_ring_radius(center, bbox_extent, cam_gt, projection);

    let yellow = Color::srgba(1.0, 1.0, 0.0, 1.0);
    const SEGS: usize = 64;

    for axis in [Axis3d::X, Axis3d::Y, Axis3d::Z] {
        let highlighted = state.dragged_axis == Some(axis);
        let base_color = axis_ring_color(axis);
        let color = if highlighted { yellow } else { base_color };
        let ring_radius = if highlighted { radius * 1.04 } else { radius };

        draw_rotation_ring(&mut gizmos, center, axis.to_vec3(), ring_radius, SEGS, color);
    }

    // Outer "free rotation" ring (white, slightly larger)
    // Gives a Roblox-style outer handle for free rotation
    let white = Color::srgba(0.9, 0.9, 0.9, 0.35);
    draw_rotation_ring(&mut gizmos, center, Vec3::ZERO, radius * 1.18, SEGS, white);
}

fn axis_ring_color(axis: Axis3d) -> Color {
    match axis {
        Axis3d::X => Color::srgba(0.95, 0.15, 0.15, 0.85),
        Axis3d::Y => Color::srgba(0.15, 0.95, 0.15, 0.85),
        Axis3d::Z => Color::srgba(0.15, 0.15, 0.95, 0.85),
    }
}

/// Draw a ring around `center` perpendicular to `axis`.
/// If `axis` is Vec3::ZERO, draws a billboard ring facing the camera (not yet implemented,
/// falls back to Y-axis ring).
fn draw_rotation_ring(
    gizmos: &mut Gizmos<TransformGizmoGroup>,
    center: Vec3,
    axis: Vec3,
    radius: f32,
    segments: usize,
    color: Color,
) {
    let axis_norm = if axis.length_squared() < 0.001 { Vec3::Y } else { axis.normalize() };

    // Build two tangent vectors perpendicular to the axis
    let up = if axis_norm.abs().dot(Vec3::Y) > 0.9 { Vec3::X } else { Vec3::Y };
    let t1 = axis_norm.cross(up).normalize();
    let t2 = axis_norm.cross(t1).normalize();

    for i in 0..segments {
        let a0 = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let a1 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
        let p0 = center + t1 * a0.cos() * radius + t2 * a0.sin() * radius;
        let p1 = center + t1 * a1.cos() * radius + t2 * a1.sin() * radius;
        gizmos.line(p0, p1, color);
    }
}

// ============================================================================
// 4. Mouse Interaction
// ============================================================================

fn handle_rotate_interaction(
    mut state: ResMut<RotateToolState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
    mut query: Query<(Entity, &GlobalTransform, &mut Transform, Option<&mut crate::classes::BasePart>), With<Selected>>,
    parent_query: Query<&ChildOf>,
    mut undo_stack: ResMut<crate::undo::UndoStack>,
    editor_settings: Res<crate::editor_settings::EditorSettings>,
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
    studio_state: Option<Res<crate::ui::StudioState>>,
    pivot_state: Option<Res<crate::pivot_mode::PivotState>>,
) {
    if !state.active { return; }

    // Transform mode governs whether rotation axes are world-aligned
    // (World) or rotated to match the active entity (Local). Must match
    // what `rotate_handles::sync_rotate_handle_root` renders.
    let transform_mode = studio_state
        .as_ref()
        .map(|s| s.transform_mode)
        .unwrap_or(crate::ui::TransformMode::World);

    // Escape cancels an in-progress rotation and restores pre-drag transforms.
    if keys.just_pressed(KeyCode::Escape) {
        if state.dragged_axis.is_some() {
            for (entity, _, mut transform, bp_opt) in query.iter_mut() {
                if let Some(pos) = state.initial_positions.get(&entity).copied() {
                    transform.translation = pos;
                }
                if let Some(rot) = state.initial_rotations.get(&entity).copied() {
                    transform.rotation = rot;
                }
                if let Some(mut bp) = bp_opt {
                    if let Some(pos) = state.initial_positions.get(&entity).copied() {
                        bp.cframe.translation = pos;
                    }
                    if let Some(rot) = state.initial_rotations.get(&entity).copied() {
                        bp.cframe.rotation = rot;
                    }
                }
            }
            state.dragged_axis = None;
            state.initial_rotations.clear();
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

    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => std::f32::consts::FRAC_PI_4,
    };

    if mouse.just_pressed(MouseButton::Left) {
        if query.is_empty() { return; }

        // Collect snapshot before mutating
        let snapshot: Vec<(Entity, Vec3, Quat, Vec3)> = query.iter()
            .map(|(e, gt, t, _)| {
                let tr = gt.compute_transform();
                (e, tr.translation, t.rotation, tr.scale)
            })
            .collect();

        // Compute group center
        let mut bmin = Vec3::splat(f32::MAX);
        let mut bmax = Vec3::splat(f32::MIN);
        for (_, pos, rot, scale) in &snapshot {
            let (mn, mx) = calculate_rotated_aabb(*pos, *scale * 0.5, *rot);
            bmin = bmin.min(mn); bmax = bmax.max(mx);
        }
        let group_center = (bmin + bmax) * 0.5;
        // Phase-1 pivot-mode integration — resolve the effective pivot
        // point through PivotState. Active → use the first-selected
        // entity's origin; Cursor → use the user-placed 3D cursor;
        // Median (default) → group center. Individual is handled by
        // the drag-math path below (rotates each entity around its own
        // origin instead of a shared center).
        let active_pos = snapshot.iter().next().map(|(_, p, _, _)| *p);
        let center = if let Some(p) = pivot_state.as_deref() {
            crate::pivot_mode::resolve_group_pivot(p, group_center, active_pos)
        } else {
            group_center
        };
        let radius = compute_ring_radius(center, bmax - bmin, camera_transform, projection);

        // Gizmo rotation — captures whether we're in World or Local mode.
        // Hit test rotates the canonical ring axes into this frame so
        // clicking the visible Local-mode ring actually registers.
        let gizmo_rotation = crate::move_tool::gizmo_rotation_for(
            transform_mode,
            snapshot.iter().map(|(_, _, r, _)| *r),
        );

        if let Some(axis) = detect_ring_hit(&ray, center, radius, gizmo_rotation) {
            state.dragged_axis = Some(axis);
            state.group_center = center;
            // Capture the gizmo rotation for stable axis across the whole
            // drag — prevents feedback loops in Local mode.
            state.drag_rotation = gizmo_rotation;
            state.drag_start_angle = angle_on_ring(&ray, center, axis, gizmo_rotation);
            state.drag_start_pos = cursor_pos;

            state.initial_rotations.clear();
            state.initial_positions.clear();
            for (entity, pos, rot, _) in &snapshot {
                state.initial_rotations.insert(*entity, *rot);
                state.initial_positions.insert(*entity, *pos);
            }
        }
    } else if mouse.pressed(MouseButton::Left) {
        if let Some(axis) = state.dragged_axis {
            let center = state.group_center;
            let gizmo_rotation = state.drag_rotation;
            let current_angle = angle_on_ring(&ray, center, axis, gizmo_rotation);
            let raw_delta = current_angle - state.drag_start_angle;

            // Angular snap: default from EditorSettings, 1° with Shift,
            // no snap with Ctrl held (CAD-style temporary snap override).
            // Defaults: 15° if unset.
            let shift_pressed = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
            let ctrl_pressed  = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
            let snap_deg = if ctrl_pressed {
                0.0_f32  // bypass snap — CAD-style temporary override
            } else if shift_pressed {
                1.0_f32
            } else {
                editor_settings.angle_snap.max(0.0)
            };
            let snapped_delta = if snap_deg > 0.0 {
                let snap_rad = snap_deg.to_radians();
                (raw_delta / snap_rad).round() * snap_rad
            } else {
                raw_delta
            };

            // Rotation axis = canonical axis rotated into gizmo frame.
            let rotation_axis = gizmo_rotation * axis.to_vec3();
            let rotation_delta = Quat::from_axis_angle(rotation_axis, snapped_delta);

            let selected_set: std::collections::HashSet<Entity> = query.iter().map(|(e, ..)| e).collect();

            // Phase-1 Individual pivot — each entity rotates around its
            // own origin instead of the group center. Other pivot modes
            // (Median / Active / Cursor) all rotate around the shared
            // `center` which was resolved at mouse-press time.
            let individual_pivot = pivot_state.as_deref()
                .map(|p| p.mode == crate::pivot_mode::PivotMode::Individual)
                .unwrap_or(false);

            for (entity, _, mut transform, basepart_opt) in query.iter_mut() {
                if is_descendant(entity, &selected_set, &parent_query) { continue; }

                if let (Some(init_rot), Some(init_pos)) = (
                    state.initial_rotations.get(&entity),
                    state.initial_positions.get(&entity),
                ) {
                    let (new_pos, new_rot) = if individual_pivot {
                        (*init_pos, rotation_delta * *init_rot)
                    } else {
                        let rel = *init_pos - center;
                        (center + rotation_delta * rel, rotation_delta * *init_rot)
                    };

                    transform.translation = new_pos;
                    transform.rotation = new_rot;

                    if let Some(mut bp) = basepart_opt {
                        bp.cframe.translation = new_pos;
                        bp.cframe.rotation = new_rot;
                    }
                }
            }
        }
    } else if mouse.just_released(MouseButton::Left) {
        if state.dragged_axis.is_some() && !state.initial_rotations.is_empty() {
            let mut old_transforms = Vec::new();
            let mut new_transforms = Vec::new();

            for (entity, _, transform, _) in query.iter() {
                if let (Some(init_rot), Some(init_pos)) = (
                    state.initial_rotations.get(&entity),
                    state.initial_positions.get(&entity),
                ) {
                    let rot_changed = init_rot.angle_between(transform.rotation) > 0.001;
                    let pos_changed = (*init_pos - transform.translation).length() > 0.001;
                    if rot_changed || pos_changed {
                        old_transforms.push((entity.to_bits(), init_pos.to_array(), init_rot.to_array()));
                        new_transforms.push((entity.to_bits(), transform.translation.to_array(), transform.rotation.to_array()));
                    }
                }
            }

            if !old_transforms.is_empty() {
                undo_stack.push(crate::undo::Action::TransformEntities { old_transforms, new_transforms });
            }
        }

        state.dragged_axis = None;
        state.initial_rotations.clear();
        state.initial_positions.clear();
    }
}

/// Apply a `NumericInputCommittedEvent` to an in-progress Rotate drag.
/// `value` is interpreted as an angle in **degrees** around the dragged
/// ring axis; absolute = total angle from drag start, relative adds
/// the same `value` on top (v1 treats the two identically since the
/// cursor-derived delta isn't carried into the finalize — Enter always
/// defines the exact angle from the initial orientation).
///
/// Matches mouse-release: pushes a `TransformEntities` undo for every
/// entity that actually moved or rotated. TOML persistence for rotate
/// is handled elsewhere — not duplicated here.
#[allow(clippy::too_many_arguments)]
fn finalize_numeric_input_on_rotate(
    mut committed: MessageReader<crate::numeric_input::NumericInputCommittedEvent>,
    mut state: ResMut<RotateToolState>,
    mut query: Query<(
        Entity,
        &mut Transform,
        Option<&mut crate::classes::BasePart>,
    ), With<Selected>>,
    mut undo_stack: ResMut<crate::undo::UndoStack>,
    parent_query: Query<&ChildOf>,
) {
    use crate::numeric_input::NumericInputOwner;

    for event in committed.read() {
        if event.owner != NumericInputOwner::Rotate { continue; }
        let Some(axis) = state.dragged_axis else { continue; };
        if state.initial_rotations.is_empty() { continue; }

        let angle_rad = event.value.to_radians();
        let rotation_axis = state.drag_rotation * axis.to_vec3();
        let rotation_delta = Quat::from_axis_angle(rotation_axis, angle_rad);
        let center = state.group_center;

        let selected_set: std::collections::HashSet<Entity> = query.iter().map(|(e, ..)| e).collect();

        let mut old_transforms = Vec::new();
        let mut new_transforms = Vec::new();

        for (entity, mut transform, basepart_opt) in query.iter_mut() {
            if is_descendant(entity, &selected_set, &parent_query) { continue; }

            let Some(init_rot) = state.initial_rotations.get(&entity).copied() else { continue };
            let Some(init_pos) = state.initial_positions.get(&entity).copied() else { continue };

            let rel = init_pos - center;
            let new_pos = center + rotation_delta * rel;
            let new_rot = rotation_delta * init_rot;

            let rot_changed = init_rot.angle_between(new_rot) > 0.001;
            let pos_changed = (init_pos - new_pos).length() > 0.001;
            if rot_changed || pos_changed {
                old_transforms.push((entity.to_bits(), init_pos.to_array(), init_rot.to_array()));
                new_transforms.push((entity.to_bits(), new_pos.to_array(),  new_rot.to_array()));
            }

            transform.translation = new_pos;
            transform.rotation    = new_rot;
            if let Some(mut bp) = basepart_opt {
                bp.cframe.translation = new_pos;
                bp.cframe.rotation    = new_rot;
            }
        }

        if !old_transforms.is_empty() {
            undo_stack.push(crate::undo::Action::TransformEntities { old_transforms, new_transforms });
        }

        state.dragged_axis = None;
        state.initial_rotations.clear();
        state.initial_positions.clear();
    }
}

// ============================================================================
// 5. Public Helpers
// ============================================================================

/// Compute the ring radius for rotation gizmos given group center, bounding extent,
/// camera transform, and projection. Ensures the ring wraps around the object
/// while maintaining a minimum screen-space presence at distance.
pub fn compute_ring_radius(
    center: Vec3,
    bbox_extent: Vec3,
    cam_gt: &GlobalTransform,
    projection: &Projection,
) -> f32 {
    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => std::f32::consts::FRAC_PI_4,
    };
    let dist = (center - cam_gt.translation()).length().max(0.1);
    // Camera-distance minimum so the ring is always visible
    let cam_radius = dist * (fov * 0.5).tan() * 0.18;
    // Object bounding sphere radius (half diagonal of bounding box)
    let object_radius = bbox_extent.length() * 0.5;
    // Use the larger of the two, with some padding so the ring wraps outside the object
    (object_radius * 1.15).max(cam_radius)
}

/// Check if the ray hits any rotation ring. Used by part_selection to avoid
/// deselecting when clicking a ring handle.
///
/// `rotation` is the gizmo's world rotation — `Quat::IDENTITY` for World
/// transform mode, or the active entity's rotation for Local mode.
/// Must match what rotate_handles::sync_rotate_handle_root renders.
pub fn is_clicking_rotate_handle(
    ray: &Ray3d,
    center: Vec3,
    radius: f32,
    _camera_transform: &GlobalTransform,
    rotation: Quat,
) -> bool {
    detect_ring_hit(ray, center, radius, rotation).is_some()
}

// ============================================================================
// Private Helpers
// ============================================================================

/// Detect which ring axis the ray hits. Returns the closest axis whose ring
/// plane intersection falls within [inner_r, outer_r] of the ring. Ring
/// normals are rotated by `rotation` so Local-mode rings are hit correctly.
fn detect_ring_hit(ray: &Ray3d, center: Vec3, radius: f32, rotation: Quat) -> Option<Axis3d> {
    let inner = radius * 0.75;
    let outer = radius * 1.25;

    let mut best: Option<(Axis3d, f32)> = None;

    for axis in [Axis3d::X, Axis3d::Y, Axis3d::Z] {
        let axis_vec = rotation * axis.to_vec3();
        if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, center, axis_vec) {
            let hit = ray.origin + *ray.direction * t;
            let dist = (hit - center).length();
            if dist >= inner && dist <= outer {
                let ring_err = (dist - radius).abs();
                if best.map_or(true, |(_, d)| ring_err < d) {
                    best = Some((axis, ring_err));
                }
            }
        }
    }

    best.map(|(a, _)| a)
}

/// Calculate the angle of the ray's intersection with the ring plane around
/// `axis` rotated by `rotation`.
fn angle_on_ring(ray: &Ray3d, center: Vec3, axis: Axis3d, rotation: Quat) -> f32 {
    let axis_vec = rotation * axis.to_vec3();
    let t = ray_plane_intersection(ray.origin, *ray.direction, center, axis_vec).unwrap_or(0.0);
    let hit = ray.origin + *ray.direction * t;
    let to_hit = hit - center;

    let up = if axis_vec.abs().dot(Vec3::Y) > 0.9 { Vec3::X } else { Vec3::Y };
    let t1 = axis_vec.cross(up).normalize();
    let t2 = axis_vec.cross(t1).normalize();

    to_hit.dot(t2).atan2(to_hit.dot(t1))
}

/// Compute the world-space center and extent of the combined AABB of all selected entities.
fn compute_group_center_and_extent(
    query: &Query<(Entity, &GlobalTransform, Option<&crate::classes::BasePart>), With<Selected>>,
    children_query: &Query<&Children>,
    child_transforms: &Query<(&GlobalTransform, Option<&crate::classes::BasePart>), Without<Selected>>,
) -> (Vec3, Vec3) {
    let mut bmin = Vec3::splat(f32::MAX);
    let mut bmax = Vec3::splat(f32::MIN);
    let mut cnt = 0;

    for (entity, gt, bp) in query.iter() {
        let t = gt.compute_transform();
        let s = bp.map(|b| b.size).unwrap_or(t.scale);
        let (mn, mx) = calculate_rotated_aabb(t.translation, s * 0.5, t.rotation);
        bmin = bmin.min(mn); bmax = bmax.max(mx); cnt += 1;

        if let Ok(children) = children_query.get(entity) {
            for child in children.iter() {
                if let Ok((cg, cbp)) = child_transforms.get(child) {
                    let ct = cg.compute_transform();
                    let cs = cbp.map(|b| b.size).unwrap_or(ct.scale);
                    let (cn, cx) = calculate_rotated_aabb(ct.translation, cs * 0.5, ct.rotation);
                    bmin = bmin.min(cn); bmax = bmax.max(cx); cnt += 1;
                }
            }
        }
    }

    if cnt == 0 {
        (Vec3::ZERO, Vec3::ONE)
    } else {
        ((bmin + bmax) * 0.5, bmax - bmin)
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
