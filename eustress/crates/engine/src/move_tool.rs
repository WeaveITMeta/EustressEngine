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
use avian3d::prelude::SpatialQuery;
use crate::selection_box::Selected;
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
    /// Which axis handle is being dragged (None = free drag, plane drag, or idle)
    pub dragged_axis: Option<Axis3d>,
    /// Which plane handle is being dragged — identified by the plane's
    /// normal axis (Axis::Z → XY plane). None unless plane-handle drag
    /// is active. Mutually exclusive with `dragged_axis`.
    pub dragged_plane: Option<Axis3d>,
    /// Which axis handle the cursor is hovering over (for visual feedback)
    pub hovered_axis: Option<Axis3d>,
    /// Which plane handle the cursor is hovering over.
    pub hovered_plane: Option<Axis3d>,
    /// Initial world positions of all selected entities at drag start
    pub initial_positions: std::collections::HashMap<Entity, Vec3>,
    /// Initial rotations of all selected entities at drag start
    pub initial_rotations: std::collections::HashMap<Entity, Quat>,
    /// Center of the combined AABB of all selected parts
    pub group_center: Vec3,
    /// Gizmo rotation captured at drag start (World = IDENTITY, Local =
    /// active entity's rotation at press-time). Kept stable for the
    /// whole drag so Local-mode drags don't feedback-loop as the
    /// entity rotates from its own motion.
    pub drag_rotation: Quat,
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
            dragged_plane: None,
            hovered_axis: None,
            hovered_plane: None,
            initial_positions: std::collections::HashMap::new(),
            initial_rotations: std::collections::HashMap::new(),
            group_center: Vec3::ZERO,
            drag_rotation: Quat::IDENTITY,
            initial_mouse_world: Vec3::ZERO,
            drag_start_pos: Vec2::ZERO,
            free_drag: false,
            dragged_entity: None,
        }
    }
}

/// World axis enum shared with rotate/scale tools. `Hash` derive is
/// needed because `rotate_handles.rs` uses `Axis3d` as a HashMap key
/// for per-axis drag state.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
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
        // `draw_move_gizmos` previously drew the move arrows via Bevy's
        // immediate-mode Gizmos<TransformGizmoGroup>, but that pipeline
        // never renders through our Slint overlay camera stack. Mesh-
        // based rendering lives in `move_handles.rs` now; this plugin
        // only handles state + drag interaction.
        // `.chain()` orders the tuple elements sequentially — which
        // is exactly what the third system wanted (`finalize_numeric_
        // input_on_move` must run after `handle_move_interaction` so
        // Enter wins over the current cursor position). Mixing an
        // `.after(...)` inside a `.chain()` tuple breaks the system-
        // set trait bounds in Bevy 0.18, so the chain ordering alone
        // carries the constraint.
        app.init_resource::<MoveToolState>()
            .add_systems(Update, (
                manage_tool_activation,
                handle_move_interaction,
                finalize_numeric_input_on_move,
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
    studio_state: Res<crate::ui::StudioState>,
    query: Query<(Entity, &GlobalTransform, Option<&crate::classes::BasePart>), With<Selected>>,
    children_query: Query<&Children>,
    child_transforms: Query<(&GlobalTransform, Option<&crate::classes::BasePart>), Without<Selected>>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
) {
    // Draw Move gizmos when Move tool is active OR Select tool is active (Roblox UX:
    // selecting an object immediately shows transform handles for visual feedback).
    let show = state.active || studio_state.current_tool == crate::ui::Tool::Select;
    if !show || query.is_empty() {
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
    let Some((_, cam_gt, projection)) = cameras.iter().find(|(c, _, _)| c.order == 0) else { return };
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
        let dragging = state.dragged_axis == Some(axis);
        let hovering = state.hovered_axis == Some(axis) && state.dragged_axis.is_none();
        let color = if dragging { yellow } else if hovering { Color::srgb(1.0, 1.0, 0.6) } else { axis.color() };

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

/// Inputs bundle for `handle_move_interaction` — groups the settings
/// + editor-context resources so the outer system stays under Bevy's
/// 16-parameter soft limit. `SystemParam` derive lets Bevy unwrap
/// these into the correct resource reads at scheduling time.
#[derive(bevy::ecs::system::SystemParam)]
pub struct MoveToolInputs<'w> {
    pub settings: Res<'w, EditorSettings>,
    pub studio_state: Option<Res<'w, crate::ui::StudioState>>,
    pub mouse: Res<'w, ButtonInput<MouseButton>>,
    pub keys: Res<'w, ButtonInput<KeyCode>>,
    pub viewport_bounds: Option<Res<'w, crate::ui::ViewportBounds>>,
    pub auth: Option<Res<'w, crate::auth::AuthState>>,
}

/// Snap-related queries + resources. Bundled for the same reason as
/// `MoveToolInputs`: keeps the per-system param count bounded.
#[derive(bevy::ecs::system::SystemParam)]
pub struct MoveToolSnapCtx<'w, 's> {
    pub spatial_query: SpatialQuery<'w, 's>,
    pub geom_snap: Option<Res<'w, crate::geom_snap::GeomSnapState>>,
    pub smart_guides: Option<Res<'w, crate::smart_guides::SmartGuidesState>>,
    pub snap_candidates_q: Query<
        'w,
        's,
        (Entity, &'static GlobalTransform, &'static crate::classes::BasePart),
        Without<Selected>,
    >,
}

fn handle_move_interaction(
    mut state: ResMut<MoveToolState>,
    inputs: MoveToolInputs,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
    mut query: Query<(Entity, &GlobalTransform, &mut Transform, Option<&mut crate::classes::BasePart>), With<Selected>>,
    children_query: Query<&Children>,
    child_global_transforms: Query<(&GlobalTransform, Option<&crate::classes::BasePart>), Without<Selected>>,
    unselected_query: Query<(Entity, &GlobalTransform, &Mesh3d, Option<&crate::rendering::PartEntity>, Option<&crate::classes::Instance>, Option<&crate::classes::BasePart>), Without<Selected>>,
    parent_query: Query<&ChildOf>,
    mut undo_stack: ResMut<crate::undo::UndoStack>,
    instance_files: Query<&crate::space::instance_loader::InstanceFile>,
    snap: MoveToolSnapCtx,
) {
    // Destructure the bundles so the rest of the body uses the same
    // local names (and same types — `Res<T>` / `Option<Res<T>>`) as
    // before the SystemParam grouping, keeping the diff minimal.
    let MoveToolInputs { settings, studio_state, mouse, keys, viewport_bounds, auth } = inputs;
    let MoveToolSnapCtx { spatial_query, geom_snap, smart_guides, snap_candidates_q } = snap;

    if !state.active { return; }

    // Read the transform mode once for this frame. World mode uses
    // literal world axes; Local mode rotates the gizmo (and therefore
    // the drag axis) by the active entity's world rotation. See
    // `gizmo_rotation_for` + `move_handles::sync_move_handle_root`.
    let transform_mode = studio_state
        .as_ref()
        .map(|s| s.transform_mode)
        .unwrap_or(crate::ui::TransformMode::World);

    // Escape cancels an in-progress drag and restores all affected
    // entities' pre-drag transforms. Matches Blender / Maya convention.
    if keys.just_pressed(KeyCode::Escape) {
        let was_dragging = state.dragged_axis.is_some()
            || state.dragged_plane.is_some()
            || state.free_drag;
        if was_dragging {
            // Restore each entity's pre-drag transform.
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
            state.dragged_plane = None;
            state.free_drag = false;
            state.dragged_entity = None;
            state.initial_positions.clear();
            state.initial_rotations.clear();
            return;
        }
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };
    
    // Block NEW drags when cursor is over UI panels (outside 3D viewport).
    // Allow in-progress drags to continue even if cursor leaves the viewport.
    // ViewportBounds is physical px, cursor_pos is logical — go through
    // contains_logical so DPI-scaled displays don't reject every click.
    if state.dragged_axis.is_none() && !state.free_drag {
        if let Some(vb) = viewport_bounds.as_deref() {
            let scale = window.scale_factor() as f32;
            if !vb.contains_logical(cursor_pos, scale) { return; }
        }
    }
    let Some((camera, camera_transform, projection)) = cameras.iter().find(|(c, _, _)| c.order == 0) else { return };
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

    // Gizmo rotation for current selection + transform mode. Shared
    // between hit-test (handles) and drag math (axis direction) so the
    // arrow you see IS the direction you drag — no disagreement.
    let gizmo_rotation = gizmo_rotation_for(
        transform_mode,
        query.iter().map(|(_, gt, _, _)| gt.compute_transform().rotation),
    );

    // ---- Hover detection (every frame) ----
    let idle = state.dragged_axis.is_none() && state.dragged_plane.is_none() && !state.free_drag;
    if idle && !query.is_empty() {
        // Plane handles take priority over axis handles when the cursor
        // is inside one — their hit zone is smaller and more specific.
        state.hovered_plane = detect_plane_hit(&ray, center, handle_len, camera_transform, gizmo_rotation);
        state.hovered_axis = if state.hovered_plane.is_some() {
            None
        } else {
            detect_axis_hit(&ray, center, handle_len, camera_transform, gizmo_rotation)
        };
    } else if !idle {
        state.hovered_axis = None;
        state.hovered_plane = None;
    }

    // ---- Mouse Down ----
    if mouse.just_pressed(MouseButton::Left) {
        if query.is_empty() { return; }

        // 1a. Check plane handle click (priority over axis — inner target).
        if let Some(normal_axis) = detect_plane_hit(&ray, center, handle_len, camera_transform, gizmo_rotation) {
            state.dragged_plane = Some(normal_axis);
            state.dragged_axis = None;
            state.free_drag = false;
            state.group_center = center;
            state.drag_start_pos = cursor_pos;
            state.drag_rotation = gizmo_rotation;

            state.initial_positions.clear();
            state.initial_rotations.clear();
            for (entity, _, transform, _) in query.iter() {
                state.initial_positions.insert(entity, transform.translation);
                state.initial_rotations.insert(entity, transform.rotation);
            }

            // Initial mouse world on the drag plane (the plane handle itself).
            let plane_normal = gizmo_rotation * normal_axis.to_vec3();
            if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, center, plane_normal) {
                state.initial_mouse_world = ray.origin + *ray.direction * t;
            }
            return;
        }

        // 1b. Check axis handle click
        if let Some(axis) = detect_axis_hit(&ray, center, handle_len, camera_transform, gizmo_rotation) {
            state.dragged_axis = Some(axis);
            state.dragged_plane = None;
            state.free_drag = false;
            state.group_center = center;
            state.drag_start_pos = cursor_pos;
            // Capture the gizmo rotation at drag-start so the drag axis
            // stays stable for the whole drag even if the part being
            // dragged rotates slightly (single-entity Local would
            // otherwise feedback-loop).
            state.drag_rotation = gizmo_rotation;

            // Store initial state for all selected parts
            state.initial_positions.clear();
            state.initial_rotations.clear();
            for (entity, _, transform, _) in query.iter() {
                state.initial_positions.insert(entity, transform.translation);
                state.initial_rotations.insert(entity, transform.rotation);
            }

            // Compute initial mouse world position on the axis drag plane.
            // Axis is rotated into the gizmo's frame for Local mode.
            let axis_vec = gizmo_rotation * axis.to_vec3();
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
        if let Some(normal_axis) = state.dragged_plane {
            // Plane-constrained drag — two-axis simultaneous movement.
            let plane_normal = state.drag_rotation * normal_axis.to_vec3();

            if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, state.group_center, plane_normal) {
                let current_world = ray.origin + *ray.direction * t;
                let raw_delta = current_world - state.initial_mouse_world;

                // Snap both in-plane tangent components independently when
                // snap is enabled. Tangent axes are the gizmo's OTHER two
                // axes (not the normal).
                let (tangent_u, tangent_v) = match normal_axis {
                    Axis3d::X => (state.drag_rotation * Vec3::Y, state.drag_rotation * Vec3::Z),
                    Axis3d::Y => (state.drag_rotation * Vec3::X, state.drag_rotation * Vec3::Z),
                    Axis3d::Z => (state.drag_rotation * Vec3::X, state.drag_rotation * Vec3::Y),
                };
                let du = raw_delta.dot(tangent_u);
                let dv = raw_delta.dot(tangent_v);
                let (sdu, sdv) = if settings.snap_enabled {
                    (
                        (du / settings.snap_size).round() * settings.snap_size,
                        (dv / settings.snap_size).round() * settings.snap_size,
                    )
                } else {
                    (du, dv)
                };
                let snapped_delta = tangent_u * sdu + tangent_v * sdv;

                let selected_set: std::collections::HashSet<Entity> = query.iter().map(|(e, ..)| e).collect();

                for (entity, _, mut transform, base_part_opt) in query.iter_mut() {
                    if is_descendant(entity, &selected_set, &parent_query) { continue; }
                    if let Some(initial_pos) = state.initial_positions.get(&entity) {
                        let new_pos = *initial_pos + snapped_delta;
                        transform.translation = new_pos;
                        if let Some(mut bp) = base_part_opt {
                            bp.cframe.translation = new_pos;
                        }
                    }
                }
            }
        } else if let Some(axis) = state.dragged_axis {
            // Axis-constrained drag. Use the rotation CAPTURED at
            // drag-start (not the current rotation), so dragging an
            // entity in Local mode doesn't feedback-loop as the entity's
            // own rotation drifts from the motion.
            let axis_vec = state.drag_rotation * axis.to_vec3();
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
            // Exclude selected entities AND their children (adornments) from raycast
            let mut selected_entities: Vec<Entity> = query.iter().map(|(e, ..)| e).collect();
            for parent in selected_entities.clone() {
                if let Ok(children) = children_query.get(parent) {
                    selected_entities.extend(children.iter());
                }
            }

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

            // Phase-1 vertex/edge/face snap — if the user is holding
            // V / E / F during drag, override the cursor-derived target
            // position with the nearest snap point on any unselected
            // part's OBB. Build candidates lazily — only when the
            // resource is present + a category is forced, to avoid
            // per-frame allocation in the common case.
            let snap_override: Option<Vec3> = if let Some(ref snap) = geom_snap {
                if snap.forced.is_some() {
                    // Cursor-world point for proximity test — raycast
                    // hit point if we have one, else the group center.
                    let probe_world = surface_hit.as_ref().map(|(p, _, _)| *p)
                        .unwrap_or(state.group_center);
                    let candidates: Vec<crate::geom_snap::SnapCandidate> = snap_candidates_q
                        .iter()
                        .map(|(e, gt, bp)| {
                            let t = gt.compute_transform();
                            crate::geom_snap::SnapCandidate {
                                entity: e,
                                transform: t,
                                size: bp.size,
                            }
                        })
                        .collect();
                    crate::geom_snap::resolve_snap_target(probe_world, snap, &candidates)
                        .map(|hit| hit.point)
                } else { None }
            } else { None };

            let target_pos = if let Some(p) = snap_override {
                p
            } else if let Some((hit_point, hit_normal, _)) = surface_hit {
                let offset = calculate_surface_offset(&leader_size, &leader_rot, &hit_normal);
                hit_point + hit_normal * offset
            } else {
                // Fallback: drag on horizontal plane
                if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, state.group_center, Vec3::Y) {
                    // Clamp t to prevent dragging into the sky producing infinity positions
                    let t = t.min(2000.0);
                    let ground = ray.origin + *ray.direction * t;
                    let offset = calculate_surface_offset(&leader_size, &leader_rot, &Vec3::Y);
                    Vec3::new(ground.x, offset, ground.z)
                } else {
                    leader_initial
                }
            };

            // Guard: reject NaN/infinity positions that crash the physics engine
            let target_pos = if target_pos.is_finite() { target_pos } else { leader_initial };

            // Phase-1 smart alignment guides — if enabled and no
            // explicit snap is already in play, check whether the
            // leader's AABB center / edges align with any unselected
            // part's plane. Per-axis override; only applied axes are
            // nudged.
            let target_pos = if snap_override.is_none() {
                if let Some(ref guides) = smart_guides {
                    if guides.enabled {
                        let (leader_min, leader_max) = calculate_rotated_aabb(
                            target_pos, leader_size * 0.5, leader_rot,
                        );
                        let snap = crate::smart_guides::resolve_guide_snap(
                            guides, leader_min, leader_max,
                        );
                        let mut t = target_pos;
                        if let Some(p) = snap.x {
                            let c = (leader_min.x + leader_max.x) * 0.5;
                            let leader_coord = match p.kind {
                                crate::smart_guides::GuideKind::Min    => leader_min.x,
                                crate::smart_guides::GuideKind::Center => c,
                                crate::smart_guides::GuideKind::Max    => leader_max.x,
                            };
                            t.x += p.value - leader_coord;
                        }
                        if let Some(p) = snap.y {
                            let c = (leader_min.y + leader_max.y) * 0.5;
                            let leader_coord = match p.kind {
                                crate::smart_guides::GuideKind::Min    => leader_min.y,
                                crate::smart_guides::GuideKind::Center => c,
                                crate::smart_guides::GuideKind::Max    => leader_max.y,
                            };
                            t.y += p.value - leader_coord;
                        }
                        if let Some(p) = snap.z {
                            let c = (leader_min.z + leader_max.z) * 0.5;
                            let leader_coord = match p.kind {
                                crate::smart_guides::GuideKind::Min    => leader_min.z,
                                crate::smart_guides::GuideKind::Center => c,
                                crate::smart_guides::GuideKind::Max    => leader_max.z,
                            };
                            t.z += p.value - leader_coord;
                        }
                        t
                    } else { target_pos }
                } else { target_pos }
            } else { target_pos };

            let final_target = if settings.snap_enabled {
                let snapped = snap_to_grid(target_pos, settings.snap_size);
                if let Some((_, hit_normal, _)) = surface_hit {
                    // Face-snap: when resting against another part's face, keep the
                    // NORMAL-axis contact exact so the AABBs meet without a gap; only
                    // snap the tangent axes to the grid. Without this, grid rounding
                    // pushes the leader off the target face (visible as a gap).
                    let n = hit_normal.normalize();
                    let flush_normal = n * target_pos.dot(n);
                    let snapped_tangent = snapped - n * snapped.dot(n);
                    snapped_tangent + flush_normal
                } else {
                    // Free-air ground drag — clamp so grid snap doesn't bury the part.
                    let min_y = target_pos.y;
                    Vec3::new(snapped.x, snapped.y.max(min_y), snapped.z)
                }
            } else {
                target_pos
            };

            let selected_set: std::collections::HashSet<Entity> = query.iter().map(|(e, ..)| e).collect();
            let pivot = leader_initial;

            // Phase-1 align-to-normal: when the user has the toggle on
            // AND we hit a surface, rotate the leader so its local +Y
            // matches the hit normal, and rotate other entities by the
            // same delta so group-relative orientation is preserved.
            let align_rotation: Option<Quat> = if settings.align_to_normal_on_drop {
                surface_hit.as_ref().and_then(|(_, normal, _)| {
                    let n = normal.normalize();
                    if n.length_squared() < 0.9 { return None; }
                    // Delta from the leader's initial local +Y to the
                    // hit normal. Expresses "rotate by this quat" —
                    // composable onto each initial rotation.
                    let initial_up = leader_rot * Vec3::Y;
                    if (initial_up - n).length_squared() < 1e-5 { return None; }
                    Some(Quat::from_rotation_arc(initial_up.normalize(), n))
                })
            } else { None };

            for (entity, _, mut transform, base_part_opt) in query.iter_mut() {
                if is_descendant(entity, &selected_set, &parent_query) { continue; }
                if let Some(initial_pos) = state.initial_positions.get(&entity) {
                    let initial_rot = state.initial_rotations.get(&entity).copied().unwrap_or(Quat::IDENTITY);
                    let rel = *initial_pos - pivot;
                    let (new_pos, new_rot) = if let Some(q) = align_rotation {
                        (final_target + q * rel, q * initial_rot)
                    } else {
                        (final_target + rel, initial_rot)
                    };
                    transform.translation = new_pos;
                    if align_rotation.is_some() {
                        transform.rotation = new_rot;
                    }
                    if let Some(mut bp) = base_part_opt {
                        bp.cframe.translation = new_pos;
                        if align_rotation.is_some() {
                            bp.cframe.rotation = new_rot;
                        }
                    }
                }
            }
        }
    }

    // ---- Mouse Released ----
    else if mouse.just_released(MouseButton::Left) {
        let was_dragging = state.dragged_axis.is_some()
            || state.dragged_plane.is_some()
            || state.free_drag;
        if was_dragging && !state.initial_positions.is_empty() {
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

            // Write updated transforms back to TOML (file-system-first persistence).
            // Every drag-release is a discrete signed event when logged in — feeds
            // the audit chain + AI "who moved what" training signal.
            let stamp = auth.as_deref().and_then(crate::space::instance_loader::current_stamp);
            for (entity, _, transform, _) in query.iter() {
                if let Ok(inst_file) = instance_files.get(entity) {
                    if let Ok(mut def) = crate::space::instance_loader::load_instance_definition(&inst_file.toml_path) {
                        def.transform.position = [transform.translation.x, transform.translation.y, transform.translation.z];
                        def.transform.rotation = [transform.rotation.x, transform.rotation.y, transform.rotation.z, transform.rotation.w];
                        let _ = crate::space::instance_loader::write_instance_definition_signed(
                            &inst_file.toml_path, &mut def, stamp.as_ref(),
                        );
                    }
                }
            }
        }

        state.dragged_axis = None;
        state.dragged_plane = None;
        state.free_drag = false;
        state.dragged_entity = None;
        state.initial_positions.clear();
        state.initial_rotations.clear();
    }
}

/// Apply a `NumericInputCommittedEvent` to an in-progress Move axis drag:
/// compute the final position as `initial + axis_vec * value` (absolute)
/// or `initial + axis_vec * (current_delta + value)` (relative — v1
/// treats it the same as absolute since we don't track current_delta
/// separately in this system), push undo + write TOML, then clear the
/// drag state. This mirrors the mouse-released code path, triggered by
/// Enter instead of LMB release.
///
/// v1 scope: axis drag only. Plane drags + free drags pass through.
#[allow(clippy::too_many_arguments)]
fn finalize_numeric_input_on_move(
    mut committed: MessageReader<crate::numeric_input::NumericInputCommittedEvent>,
    mut state: ResMut<MoveToolState>,
    mut query: Query<(Entity, &mut Transform, Option<&mut crate::classes::BasePart>), With<Selected>>,
    mut undo_stack: ResMut<crate::undo::UndoStack>,
    instance_files: Query<&crate::space::instance_loader::InstanceFile>,
    auth: Option<Res<crate::auth::AuthState>>,
) {
    use crate::numeric_input::NumericInputOwner;

    for event in committed.read() {
        if event.owner != NumericInputOwner::Move { continue; }
        let Some(axis) = state.dragged_axis else { continue; };
        if state.initial_positions.is_empty() { continue; }

        let axis_vec = state.drag_rotation * axis.to_vec3();
        let value = event.value;

        // Apply: new_pos = initial + axis_vec * value. Absolute and
        // relative collapse to the same formula here because `value`
        // is already the user's typed delta along the axis from drag
        // start. (If we later track live cursor-delta as a base, a
        // relative entry would add to it.)
        let mut old_transforms = Vec::new();
        let mut new_transforms = Vec::new();

        for (entity, mut transform, base_part_opt) in query.iter_mut() {
            let Some(initial_pos) = state.initial_positions.get(&entity).copied() else { continue };
            let Some(initial_rot) = state.initial_rotations.get(&entity).copied() else { continue };
            let new_pos = initial_pos + axis_vec * value;

            if (initial_pos - new_pos).length() > 0.001 {
                old_transforms.push((entity.to_bits(), initial_pos.to_array(), initial_rot.to_array()));
                new_transforms.push((entity.to_bits(), new_pos.to_array(), transform.rotation.to_array()));
            }

            transform.translation = new_pos;
            if let Some(mut bp) = base_part_opt {
                bp.cframe.translation = new_pos;
            }
        }

        if !old_transforms.is_empty() {
            undo_stack.push(crate::undo::Action::TransformEntities { old_transforms, new_transforms });
        }

        // Persist to TOML — same signed path as mouse-release.
        let stamp = auth.as_deref().and_then(crate::space::instance_loader::current_stamp);
        for (entity, transform, _) in query.iter().map(|(e, t, _)| (e, t, ())) {
            if let Ok(inst_file) = instance_files.get(entity) {
                if let Ok(mut def) = crate::space::instance_loader::load_instance_definition(&inst_file.toml_path) {
                    def.transform.position = [transform.translation.x, transform.translation.y, transform.translation.z];
                    def.transform.rotation = [transform.rotation.x, transform.rotation.y, transform.rotation.z, transform.rotation.w];
                    let _ = crate::space::instance_loader::write_instance_definition_signed(
                        &inst_file.toml_path, &mut def, stamp.as_ref(),
                    );
                }
            }
        }

        // Clear drag state — matches the mouse-released path.
        state.dragged_axis = None;
        state.dragged_plane = None;
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
///
/// `rotation` is the gizmo's world rotation — `Quat::IDENTITY` for World
/// transform mode, or the active entity's world rotation for Local mode.
/// Must match the rotation used to render the handles in
/// [`move_handles::sync_move_handle_root`], or the click target drifts
/// off the visible arrow.
pub fn detect_axis_hit(
    ray: &Ray3d,
    center: Vec3,
    handle_len: f32,
    camera_transform: &GlobalTransform,
    rotation: Quat,
) -> Option<Axis3d> {
    // Hit radius scales with handle length so small and large handles are equally clickable
    let hit_radius = (handle_len * 0.18).clamp(0.05, 0.6);

    let mut best: Option<(Axis3d, f32)> = None;

    for axis in [Axis3d::X, Axis3d::Y, Axis3d::Z] {
        // Rotate the canonical axis into the gizmo's frame so hit detection
        // lines up with the rotated arrows in Local mode.
        let dir = rotation * axis.to_vec3();
        for sign in [1.0_f32, -1.0] {
            let seg_end = center + dir * handle_len * sign;
            let dist = ray_to_line_segment_distance(ray.origin, *ray.direction, center, seg_end);
            if dist < hit_radius {
                // Depth-sort: prefer the handle closest to the camera
                let mid = (center + seg_end) * 0.5;
                let cam_dist = (mid - camera_transform.translation()).length();
                if best.map_or(true, |(_, d)| cam_dist < d) {
                    best = Some((axis, cam_dist));
                }
            }
        }
    }

    best.map(|(a, _)| a)
}

/// Public wrapper used by part_selection and select_tool to avoid interfering with move handles.
///
/// Pass `Quat::IDENTITY` as `rotation` for world-axis handles; pass the
/// active entity's rotation for Local mode. Must match
/// [`move_handles::sync_move_handle_root`]'s rotation choice to keep
/// click detection aligned with the visible gizmo.
pub fn is_clicking_move_handle(
    ray: &Ray3d,
    center: Vec3,
    _size: Vec3,
    handle_len: f32,
    camera_transform: &GlobalTransform,
    rotation: Quat,
) -> bool {
    detect_axis_hit(ray, center, handle_len, camera_transform, rotation).is_some()
}

/// Plane-handle world placement used by both hit-test and visual layout.
/// The plane handle's normal is `rotation × normal_axis`, and it sits at
/// `center + rotation × offset × handle_len`, where `offset` is the
/// plane's root-local offset (e.g. `(0.3, 0.3, 0)` for XY).
pub struct PlaneHandlePlacement {
    pub normal_axis: Axis3d,
    pub plane_center: Vec3,
    pub plane_normal: Vec3,
    /// Two in-plane tangent directions; half-extent along each is
    /// `half_size` in world units.
    pub tangent_u: Vec3,
    pub tangent_v: Vec3,
    pub half_size: f32,
}

/// Compute world-space placement for the 3 plane handles. `handle_len`
/// is the Move gizmo's camera-distance scale (same as axis arrow
/// length). Must match the layout constants in `move_handles.rs`.
pub fn plane_handle_placements(
    center: Vec3,
    handle_len: f32,
    rotation: Quat,
) -> [PlaneHandlePlacement; 3] {
    // These constants mirror `move_handles::PLANE_HANDLE_OFFSET` and
    // `PLANE_HANDLE_SIZE` exactly — keep them in lockstep.
    const PLANE_OFFSET_LOCAL: f32 = 0.30;
    const PLANE_SIZE_LOCAL: f32 = 0.22;

    let offset_world = PLANE_OFFSET_LOCAL * handle_len;
    let half_size = PLANE_SIZE_LOCAL * handle_len * 0.5;

    [
        // XY plane (normal = Z)
        PlaneHandlePlacement {
            normal_axis: Axis3d::Z,
            plane_center: center + rotation * Vec3::new(offset_world, offset_world, 0.0),
            plane_normal: rotation * Vec3::Z,
            tangent_u: rotation * Vec3::X,
            tangent_v: rotation * Vec3::Y,
            half_size,
        },
        // XZ plane (normal = Y)
        PlaneHandlePlacement {
            normal_axis: Axis3d::Y,
            plane_center: center + rotation * Vec3::new(offset_world, 0.0, offset_world),
            plane_normal: rotation * Vec3::Y,
            tangent_u: rotation * Vec3::X,
            tangent_v: rotation * Vec3::Z,
            half_size,
        },
        // YZ plane (normal = X)
        PlaneHandlePlacement {
            normal_axis: Axis3d::X,
            plane_center: center + rotation * Vec3::new(0.0, offset_world, offset_world),
            plane_normal: rotation * Vec3::X,
            tangent_u: rotation * Vec3::Y,
            tangent_v: rotation * Vec3::Z,
            half_size,
        },
    ]
}

/// Returns the normal-axis of the plane handle the ray hits, or None.
/// Planes are laid out by `plane_handle_placements` and tested in order;
/// the closest hit wins.
pub fn detect_plane_hit(
    ray: &Ray3d,
    center: Vec3,
    handle_len: f32,
    camera_transform: &GlobalTransform,
    rotation: Quat,
) -> Option<Axis3d> {
    let mut best: Option<(Axis3d, f32)> = None;

    for p in plane_handle_placements(center, handle_len, rotation).iter() {
        // Ray vs plane
        let Some(t) = ray_plane_intersection(
            ray.origin, *ray.direction, p.plane_center, p.plane_normal,
        ) else { continue };
        if t < 0.0 { continue; }

        let hit = ray.origin + *ray.direction * t;
        let rel = hit - p.plane_center;

        // Hit point's distance along each tangent — inside the square
        // if both magnitudes are within half_size.
        let du = rel.dot(p.tangent_u).abs();
        let dv = rel.dot(p.tangent_v).abs();

        if du <= p.half_size && dv <= p.half_size {
            let cam_dist = (p.plane_center - camera_transform.translation()).length();
            if best.map_or(true, |(_, d)| cam_dist < d) {
                best = Some((p.normal_axis, cam_dist));
            }
        }
    }

    best.map(|(ax, _)| ax)
}

/// Compute the gizmo's world rotation given the current transform mode
/// and the currently-selected entities. Centralized here so every system
/// (move_tool drag, move_handles visual, part_selection hit test,
/// select_tool hit test) agrees on the same rotation — a mismatch
/// between visual and hit-test was previously possible.
///
/// World mode → always `Quat::IDENTITY`.
/// Local mode + empty selection → `Quat::IDENTITY` (no entity to frame on).
/// Local mode + N entities → the LAST entity's rotation per the
/// "active entity" convention from Maya / Unity / Roblox.
pub fn gizmo_rotation_for(
    transform_mode: crate::ui::TransformMode,
    selected_rotations: impl IntoIterator<Item = Quat>,
) -> Quat {
    match transform_mode {
        crate::ui::TransformMode::World => Quat::IDENTITY,
        crate::ui::TransformMode::Local => {
            // Last entity in iteration order is the "active" one. Bevy
            // query order isn't selection-order-stable across frames,
            // but within a frame it's deterministic — fine for our
            // use-case since Local rotation is visual-only.
            selected_rotations.into_iter().last().unwrap_or(Quat::IDENTITY)
        }
    }
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
