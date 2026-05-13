//! # Select Tool
//!
//! The Select Tool provides base behavior for all transformation tools:
//! - Click to select entities
//! - Drag to move selected entities
//! - R key to rotate 90° on Y axis
//! - T key to tilt 90° on Z axis
//! - Box selection for multiple entities
//! - Physics-based surface detection via Avian3D
//! - Grid snapping support
//!
//! Other tools (Move, Scale, Rotate) inherit this base behavior and add
//! their specific gizmos and interaction modes.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use avian3d::prelude::*;
use crate::selection_box::Selected;
use crate::rendering::PartEntity;
use crate::classes::{BasePart, Instance};
use crate::ui::{StudioState, Tool, BevySelectionManager, SlintUIFocus};
use crate::math_utils::{
    calculate_rotated_aabb, ray_plane_intersection, ray_obb_intersection,
    ray_intersects_part,
    find_surface_with_physics as math_find_surface_with_physics,
    find_surface_under_cursor_with_normal as math_find_surface_with_normal,
    calculate_surface_offset as math_calculate_surface_offset,
    snap_to_grid as math_snap_to_grid,
    snap_to_grid_in_frame as math_snap_to_grid_in_frame,
    face_snap_offset as math_face_snap_offset,
    find_face_contact as math_find_face_contact,
    FACE_SNAP_THRESHOLD,
};

/// Drag threshold in pixels - must move this far to start dragging
const DRAG_THRESHOLD: f32 = 5.0;

/// Box selection threshold - must drag this far to start box select (in pixels)
const BOX_SELECT_THRESHOLD: f32 = 3.0;

/// Resource tracking the select tool drag state
#[derive(Resource)]
pub struct SelectToolState {
    pub dragging: bool,
    pub drag_started: bool, // Track if drag threshold was exceeded
    pub dragged_entity: Option<Entity>,
    pub drag_offset: Vec3,
    pub initial_position: Vec3,
    pub initial_cursor_pos: Vec2, // Track initial cursor position for threshold
    /// World-space point on the part where the user originally clicked.
    /// `grab_offset_local = part_rot.inverse() * (grab_world - part_center)`.
    /// During drag, the cursor's surface hit should equal
    /// `part_center + part_rot * grab_offset_local` — solve for part_center.
    /// This is what makes the part feel "stuck to the cursor" on the exact
    /// pixel the user grabbed (Roblox-style), not jumping its center to
    /// the cursor.
    pub grab_offset_local: Vec3,
    pub initial_positions: std::collections::HashMap<Entity, Vec3>, // Store all selected parts' positions
    pub initial_rotations: std::collections::HashMap<Entity, Quat>, // Store all selected parts' rotations
    // Group bounding box for multi-selection
    pub group_center: Vec3,
    pub group_bounds_min: Vec3,
    pub group_bounds_max: Vec3,
    pub group_size: Vec3, // Size of the bounding box
    // Smoothing state for stable dragging
    pub last_target_position: Vec3, // Last calculated target position
    pub last_surface_normal: Vec3,  // Cache the last valid surface normal
    pub last_hit_entity: Option<Entity>, // Cache the last hit entity for stability
    // Debug visualization
    pub debug_hit_point: Option<Vec3>,
    pub debug_hit_normal: Option<Vec3>,
}

impl Default for SelectToolState {
    fn default() -> Self {
        Self {
            dragging: false,
            drag_started: false,
            dragged_entity: None,
            drag_offset: Vec3::ZERO,
            initial_position: Vec3::ZERO,
            initial_cursor_pos: Vec2::ZERO,
            grab_offset_local: Vec3::ZERO,
            initial_positions: std::collections::HashMap::new(),
            initial_rotations: std::collections::HashMap::new(),
            group_center: Vec3::ZERO,
            group_bounds_min: Vec3::ZERO,
            group_bounds_max: Vec3::ZERO,
            group_size: Vec3::ONE,
            last_target_position: Vec3::ZERO,
            last_surface_normal: Vec3::Y,
            last_hit_entity: None,
            debug_hit_point: None,
            debug_hit_normal: None,
        }
    }
}

// ============================================================================
// Box Selection State
// ============================================================================

/// Resource tracking box selection state
#[derive(Resource, Default)]
pub struct BoxSelectionState {
    /// Is box selection currently being drawn (threshold exceeded)
    pub active: bool,
    /// Is a potential box selection in progress (mouse down on empty space)
    pub pending: bool,
    /// Start position in screen space (only valid when pending or active)
    pub start_pos: Vec2,
    /// Current position in screen space
    pub current_pos: Vec2,
    /// Whether we're in additive mode (Shift held)
    pub additive: bool,
    /// Entities that were selected before box select started (for additive mode)
    pub previous_selection: Vec<Entity>,
}

/// Plugin for the select tool drag functionality
pub struct SelectToolPlugin;

impl Plugin for SelectToolPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<SelectToolState>()
            .init_resource::<BoxSelectionState>()
            .add_systems(Update, (
                handle_select_drag
                    .after(crate::ui::slint_ui::update_slint_ui_focus),
                handle_box_selection
                    .after(handle_select_drag),
                debug_drag_gizmos.after(handle_box_selection),
            ))
            // render_box_selection uses NonSend<SlintUiState> — must run separately
            // to avoid blocking the chain on main thread exclusivity
            .add_systems(Update, render_box_selection.after(handle_box_selection));
    }
}

/// Debug gizmos to visualize raycast hits and surface normals during drag
fn debug_drag_gizmos(
    mut gizmos: Gizmos,
    state: Option<Res<SelectToolState>>,
) {
    let Some(state) = state else { return };
    // Only show debug when actively dragging
    if !state.dragging || !state.drag_started {
        return;
    }
    
    // Draw hit point as a small sphere
    if let Some(hit_point) = state.debug_hit_point {
        gizmos.sphere(Isometry3d::from_translation(hit_point), 0.1, Color::srgb(0.0, 1.0, 0.0));
        
        // Draw surface normal as an arrow
        if let Some(normal) = state.debug_hit_normal {
            let arrow_end = hit_point + normal * 1.0;
            gizmos.line(hit_point, arrow_end, Color::srgb(0.0, 0.5, 1.0));
            // Arrow head
            gizmos.sphere(Isometry3d::from_translation(arrow_end), 0.05, Color::srgb(0.0, 0.5, 1.0));
        }
    }
    
    // Draw the target position
    if state.last_target_position != Vec3::ZERO {
        gizmos.sphere(Isometry3d::from_translation(state.last_target_position), 0.15, Color::srgb(1.0, 1.0, 0.0));
    }
}

/// System to handle click-and-drag for selected parts
/// 
/// This is the BASE BEHAVIOR for all tools:
/// - Drag to move selected entities
/// - R key to rotate 90° on Y axis
/// - T key to tilt 90° on Z axis
/// - Physics-based surface snapping via Avian3D
/// - Grid snapping when enabled
fn handle_select_drag(
    mut state: ResMut<SelectToolState>,
    studio_state: Option<Res<StudioState>>,
    ui_focus: Option<Res<SlintUIFocus>>,
    input: (Res<ButtonInput<MouseButton>>, Res<ButtonInput<KeyCode>>),
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
    // Support both PartEntity (legacy) and Instance (modern) components
    mut selected_query: Query<(Entity, &mut Transform, &GlobalTransform, Option<&PartEntity>, Option<&Instance>, Option<&mut BasePart>), With<Selected>>,
    all_parts_query: Query<(Entity, &GlobalTransform, &Mesh3d, Option<&PartEntity>, Option<&Instance>, Option<&BasePart>), Without<Selected>>,
    // Query for children of selected entities (for Model support)
    hierarchy_queries: (Query<&Children>, Query<&ChildOf>),
    spatial_query: SpatialQuery,
    settings_and_undo: (Res<crate::editor_settings::EditorSettings>, ResMut<crate::undo::UndoStack>),
    // Tool states to check if clicking on handles
    tool_states: (Res<crate::move_tool::MoveToolState>, Res<crate::scale_tool::ScaleToolState>, Res<crate::rotate_tool::RotateToolState>),
    // For writing transform back to TOML after drag
    instance_files: Query<&crate::space::instance_loader::InstanceFile>,
) {
    let Some(studio_state) = studio_state else { return };
    let (mouse, keys) = input;
    let (children_query, parent_query) = hierarchy_queries;
    let (editor_settings, mut undo_stack) = settings_and_undo;
    let (move_state, scale_state, rotate_state) = tool_states;
    // Active with Select, Move, Scale, or Rotate tools
    let drag_enabled = matches!(
        studio_state.current_tool,
        Tool::Select | Tool::Move | Tool::Scale | Tool::Rotate
    );
    
    if !drag_enabled {
        if state.dragging {
            state.dragging = false;
            state.dragged_entity = None;
        }
        return;
    }
    
    // Block input when Slint UI has focus (mouse is over UI panels)
    if let Some(ui_focus) = ui_focus {
        if ui_focus.has_focus {
            return;
        }
    }
    
    let Ok(window) = windows.single() else { return; };
    let Some(cursor_pos) = window.cursor_position() else { return; };
    let Some((camera, camera_transform, projection)) = cameras.iter().find(|(c, _, _)| c.order == 0) else { return; };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else { return; };
    
    if mouse.just_pressed(MouseButton::Left) {
        // Check Move tool handles FIRST (before blanket return)
        // This allows clicking on unselected objects while Move tool is active
        if move_state.active && studio_state.current_tool == Tool::Move {
            // Calculate group bounds for move handle detection (same as move_tool.rs)
            let mut bounds_min = Vec3::splat(f32::MAX);
            let mut bounds_max = Vec3::splat(f32::MIN);
            let mut count = 0;
            
            for (_entity, _, global_transform, _, _, basepart_opt) in selected_query.iter() {
                let t = global_transform.compute_transform();
                let size = basepart_opt.map(|bp| bp.size).unwrap_or(t.scale);
                let half_size = size * 0.5;
                let (part_min, part_max) = calculate_rotated_aabb(t.translation, half_size, t.rotation);
                bounds_min = bounds_min.min(part_min);
                bounds_max = bounds_max.max(part_max);
                count += 1;
            }
            
            if count > 0 {
                let center = (bounds_min + bounds_max) * 0.5;
                
                // MUST match move_tool.rs camera_scale_factor exactly!
                let fov = match projection {
                    Projection::Perspective(p) => p.fov,
                    _ => std::f32::consts::FRAC_PI_4,
                };
                let cam_dist = (center - camera_transform.translation()).length().max(0.1);
                let scale = cam_dist * (fov * 0.5).tan() * 0.16;
                let handle_length = scale * 1.0;
                
                // Check if clicking on move handle - let move_tool handle it.
                // Gizmo rotation follows StudioState.transform_mode so the
                // click target matches the visible arrow in Local mode.
                let gizmo_rotation = crate::move_tool::gizmo_rotation_for(
                    studio_state.transform_mode,
                    selected_query.iter().map(|(_, _, gt, _, _, _)| gt.compute_transform().rotation),
                );
                if crate::move_tool::is_clicking_move_handle(
                    &ray, center, Vec3::ONE, handle_length, &camera_transform, gizmo_rotation,
                ) {
                    return;
                }
                
                // Check if clicking on a selected part body - let move_tool handle free drag
                for (_entity, _, global_transform, _, _, basepart_opt) in selected_query.iter() {
                    let t = global_transform.compute_transform();
                    let size = basepart_opt.map(|bp| bp.size).unwrap_or(t.scale);
                    if crate::math_utils::ray_intersects_part_rotated(&ray, t.translation, t.rotation, size) {
                        // Clicking on selected part - move_tool handles free drag
                        return;
                    }
                }
            }
            // Not clicking on handle or selected part - continue to allow selecting new objects
        }
        
        // Check Scale tool handles (group-level, matching scale_handles.rs).
        if scale_state.active && studio_state.current_tool == Tool::Scale {
            let mut s_bmin = Vec3::splat(f32::MAX);
            let mut s_bmax = Vec3::splat(f32::MIN);
            let mut s_count = 0;
            for (_e, _, gt, _, _, bp) in selected_query.iter() {
                let t = gt.compute_transform();
                let sz = bp.map(|b| b.size).unwrap_or(t.scale);
                let (mn, mx) = calculate_rotated_aabb(t.translation, sz * 0.5, t.rotation);
                s_bmin = s_bmin.min(mn);
                s_bmax = s_bmax.max(mx);
                s_count += 1;
            }
            if s_count > 0 {
                let group_center = (s_bmin + s_bmax) * 0.5;
                let group_extent = (s_bmax - s_bmin) * 0.5;
                let scale_fov = match projection {
                    Projection::Perspective(p) => p.fov,
                    _ => std::f32::consts::FRAC_PI_4,
                };
                let screen_scale = crate::scale_tool::compute_scale_screen_scale(
                    group_center, camera_transform.translation(), scale_fov,
                );
                let scale_rotation = crate::move_tool::gizmo_rotation_for(
                    studio_state.transform_mode,
                    selected_query.iter().map(|(_, _, gt, _, _, _)| gt.compute_transform().rotation),
                );
                if crate::scale_tool::is_clicking_scale_handle_group(&ray, group_center, group_extent, screen_scale, scale_rotation) {
                    return;
                }
            }
        }
        
        // Check Rotate tool handles (group bounding box, matching rotate_tool.rs)
        if rotate_state.active && studio_state.current_tool == Tool::Rotate {
            let mut rot_bmin = Vec3::splat(f32::MAX);
            let mut rot_bmax = Vec3::splat(f32::MIN);
            let mut rot_cnt = 0;
            for (_entity, _, global_transform, _, _, basepart_opt) in selected_query.iter() {
                let t = global_transform.compute_transform();
                let size = basepart_opt.map(|bp| bp.size).unwrap_or(t.scale);
                let (mn, mx) = calculate_rotated_aabb(t.translation, size * 0.5, t.rotation);
                rot_bmin = rot_bmin.min(mn);
                rot_bmax = rot_bmax.max(mx);
                rot_cnt += 1;
            }
            if rot_cnt > 0 {
                let rot_center = (rot_bmin + rot_bmax) * 0.5;
                let rot_extent = rot_bmax - rot_bmin;
                let rotate_radius = crate::rotate_tool::compute_ring_radius(rot_center, rot_extent, &camera_transform, projection);
                let rotate_rotation = crate::move_tool::gizmo_rotation_for(
                    studio_state.transform_mode,
                    selected_query.iter().map(|(_, _, gt, _, _, _)| gt.compute_transform().rotation),
                );
                if crate::rotate_tool::is_clicking_rotate_handle(&ray, rot_center, rotate_radius, &camera_transform, rotate_rotation) {
                    return;
                }
            }
        }
        
        // No tool handle clicked - check if clicking on a selected part to start dragging
        for (entity, transform, global_transform, _part_entity, _instance, basepart_opt) in &selected_query {
            let t = global_transform.compute_transform();
            let size = basepart_opt.map(|bp| bp.size).unwrap_or(t.scale);
            // Use precise OBB intersection with rotation. Returns the ray
            // parameter `t_hit` if hit; we need it to compute the exact
            // world-space click point on the part for the grab-pivot.
            if let Some(t_hit) = crate::math_utils::ray_obb_intersection(
                ray.origin, *ray.direction, t.translation, size * 0.5, t.rotation,
            ) {
                let grab_world = ray.origin + *ray.direction * t_hit;
                // Convert to part-local frame so the offset rotates with
                // the part (in case rotation changes mid-drag) and stays
                // attached to the same physical point.
                let grab_offset_local = t.rotation.inverse() * (grab_world - t.translation);
                state.grab_offset_local = grab_offset_local;
                state.dragging = true;
                state.drag_started = false; // Not started until threshold exceeded
                state.dragged_entity = Some(entity);
                state.initial_position = transform.translation;
                state.initial_cursor_pos = cursor_pos; // Store initial cursor position
                state.drag_offset = transform.translation - ray.origin;
                
                // Store initial positions/rotations of ALL selected parts
                state.initial_positions.clear();
                state.initial_rotations.clear();
                let mut bounds_min = Vec3::splat(f32::MAX);
                let mut bounds_max = Vec3::splat(f32::MIN);
                
                for (sel_entity, sel_transform, _, _, _, sel_basepart_opt) in selected_query.iter() {
                    state.initial_positions.insert(sel_entity, sel_transform.translation);
                    state.initial_rotations.insert(sel_entity, sel_transform.rotation);
                    
                    // Calculate this part's AABB contribution to the group bounds
                    let part_size = sel_basepart_opt.map(|bp| bp.size).unwrap_or(sel_transform.scale);
                    let half_size = part_size * 0.5;
                    
                    // Get rotated extents for accurate bounding box
                    let (part_min, part_max) = calculate_rotated_aabb(
                        sel_transform.translation,
                        half_size,
                        sel_transform.rotation
                    );
                    
                    bounds_min = bounds_min.min(part_min);
                    bounds_max = bounds_max.max(part_max);
                }
                
                // Store group bounding box
                state.group_bounds_min = bounds_min;
                state.group_bounds_max = bounds_max;
                state.group_center = (bounds_min + bounds_max) * 0.5;
                state.group_size = bounds_max - bounds_min;
                
                return;
            }
        }
    } else if mouse.pressed(MouseButton::Left) && state.dragging {
        // PRIORITY: When Move tool is active, it handles ALL dragging
        // Cancel any select_tool drag and let move_tool take over
        if move_state.active && studio_state.current_tool == Tool::Move {
            state.dragging = false;
            state.drag_started = false;
            state.dragged_entity = None;
            return;
        }
        
        // Check if we've exceeded the drag threshold
        if !state.drag_started {
            let drag_distance = (cursor_pos - state.initial_cursor_pos).length();
            if drag_distance < DRAG_THRESHOLD {
                return; // Not enough movement yet - don't start dragging
            }
            // Threshold exceeded - start actual dragging
            state.drag_started = true;
        }
        
        // Continue dragging (only if threshold was exceeded)
        if state.drag_started {
            if let Some(dragged_entity) = state.dragged_entity {
                // Get list of selected entities to exclude from raycasting
                let mut excluded_entities: Vec<Entity> = selected_query.iter()
                    .map(|(e, _, _, _, _, _)| e)
                    .collect();
                // Also exclude children of selected entities (selection adornments, etc.)
                // to prevent wireframe meshes from interfering with surface raycasting
                let parent_list = excluded_entities.clone();
                for parent in &parent_list {
                    if let Ok(children) = children_query.get(*parent) {
                        excluded_entities.extend(children.iter());
                    }
                }

                // Retrieve leader initial state
                let initial_leader_pos = state.initial_positions.get(&dragged_entity).cloned().unwrap_or(Vec3::ZERO);
                let initial_leader_rot = state.initial_rotations.get(&dragged_entity).cloned().unwrap_or(Quat::IDENTITY);

                // We need the leader's size for offset calculation. When BasePart
                // is missing (Models, custom-mesh imports, partially-loaded
                // entities) fall through to Transform.scale — that's the same
                // fallback every other tool in this file uses (lines 230, 266,
                // 283, 316, 339, 945). Without this match, dragging a custom-
                // mesh part onto another part used Vec3::ONE for the offset,
                // which placed taller parts half-embedded into their host.
                let leader_size = if let Ok((_, t, _, _, _, basepart_opt)) = selected_query.get(dragged_entity) {
                    basepart_opt.map(|bp| bp.size).unwrap_or(t.scale)
                } else {
                    Vec3::ONE
                };
                
                // 1. Find Surface (Roblox-style: cursor surface drives the drop)
                let surface_hit = math_find_surface_with_physics(&spatial_query, &ray, &excluded_entities)
                    .map(|(pt, norm, ent)| (pt, norm, Some(ent)))
                    .or_else(|| math_find_surface_with_normal(&ray, &all_parts_query, &excluded_entities)
                        .map(|(pt, norm)| (pt, norm, None)));

                // The grab pivot rotated into the current world frame.
                // Drag math: the cursor's surface hit point should land on
                // the SAME physical spot of the part the user grabbed —
                // i.e. `cursor_world = center + rot * grab_offset_local`.
                // Solving: `center = cursor_world - rot * grab_offset_local`.
                let grab_offset_world = initial_leader_rot * state.grab_offset_local;

                let mut surface_frame: Option<(Quat, Vec3, Vec3)> = None;
                let target_pos = if let Some((hit_point, hit_normal, hit_entity)) = surface_hit {
                    // ── Surface follow ───────────────────────────────────
                    state.last_hit_entity = hit_entity;
                    state.last_surface_normal = hit_normal;
                    state.debug_hit_point = Some(hit_point);
                    state.debug_hit_normal = Some(hit_normal);

                    // Center if we glued the cursor's grabbed point to the surface hit.
                    let cursor_pivot_center = hit_point - grab_offset_world;

                    // But we also want the part's BOTTOM (the face whose outward
                    // normal is closest to -hit_normal) to rest flush on the
                    // surface. Compute the offset along the surface normal that
                    // makes that happen, then mix: keep the cursor-pivot
                    // tangent components, lift to flush along the normal.
                    let offset = math_calculate_surface_offset(&leader_size, &initial_leader_rot, &hit_normal);
                    let flush_along_normal = hit_point + hit_normal * offset;
                    // Strip the grab_offset_world's component along the
                    // surface normal so the part's contact face sits ON the
                    // surface, not above/below by the grab's vertical
                    // offset. The tangent (sliding along the surface) part
                    // of the grab pivot is preserved so the grabbed corner
                    // tracks the cursor along the face.
                    let grab_tangent = grab_offset_world - hit_normal * grab_offset_world.dot(hit_normal);
                    let flush_pivot_center = flush_along_normal - grab_tangent;

                    // Choose the flush variant — cursor stays on the surface,
                    // part contact face is flush, grabbed point still tracks
                    // along the surface plane.
                    let _ = cursor_pivot_center;
                    let target = flush_pivot_center;

                    // Capture target frame for in-frame grid snap.
                    if let Some(target_entity) = hit_entity {
                        if let Ok((_, target_xform, _, _, _, target_basepart)) = all_parts_query.get(target_entity) {
                            let target_size = target_basepart.map(|bp| bp.size).unwrap_or(Vec3::ONE);
                            let target_xf = target_xform.compute_transform();
                            surface_frame = Some((target_xf.rotation, target_xf.translation, hit_normal));

                            // Edge-snap: when grabbed-corner is within
                            // FACE_SNAP_THRESHOLD of a target corner, pull
                            // it onto the corner so adjacent parts butt
                            // cleanly. Operates on the part center.
                            let snap = math_face_snap_offset(
                                leader_size,
                                initial_leader_rot,
                                target,
                                target_size,
                                target_xf.rotation,
                                target_xf.translation,
                                hit_normal,
                                FACE_SNAP_THRESHOLD,
                            );
                            target + snap
                        } else { target }
                    } else { target }
                } else {
                    // ── No drop on empty space ───────────────────────────
                    // Cursor is over the skybox. Roblox keeps the part at
                    // its current height and slides it on the horizontal
                    // plane through its initial Y. Don't fall to Y=0.
                    //
                    // The initial Y is the part's CURRENT height, NOT the
                    // grid-snapped one — but the grid snap below WILL
                    // round it to a clean Y (e.g. 1.755 → 2.0 with
                    // snap_size=1). That's exactly what the user wants:
                    // off-grid parts converge to the grid as soon as
                    // they're dragged. Earlier code locked Y at the
                    // initial value, blocking the snap from ever fixing
                    // off-grid drift.
                    state.debug_hit_point = None;
                    state.debug_hit_normal = None;

                    let plane_y = initial_leader_pos.y;
                    if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction,
                        Vec3::new(0.0, plane_y, 0.0), Vec3::Y)
                    {
                        let t = t.clamp(0.0, 2000.0);
                        let cursor_world = ray.origin + *ray.direction * t;
                        // Glue the grabbed point to the cursor on this
                        // horizontal plane. Y stays at plane_y for now;
                        // the world-frame grid snap below will lock it
                        // to a clean grid Y on every frame.
                        Vec3::new(
                            cursor_world.x - grab_offset_world.x,
                            plane_y,
                            cursor_world.z - grab_offset_world.z,
                        )
                    } else {
                        initial_leader_pos
                    }
                };

                // Guard: reject NaN/infinity positions that crash the physics engine
                let target_pos = if target_pos.is_finite() { target_pos } else { initial_leader_pos };

                // No rotation change during drag
                let rotation_delta = Quat::IDENTITY;

                // Apply grid snapping if enabled.
                //
                // Three cases:
                //
                // 1. Surface hit (cursor over a part) — snap the grabbed
                //    point IN THE TARGET'S LOCAL FRAME, but preserve the
                //    surface-normal component so the dragged part stays
                //    flush. This is the existing `snap_to_grid_in_frame`
                //    behaviour. Operates on the grabbed point so corners
                //    click cleanly to grid intersections.
                //
                // 2. Empty space hit (cursor over skybox) — snap the
                //    grabbed point in WORLD frame on ALL THREE axes
                //    (including Y). Earlier code preserved the initial
                //    Y so off-grid parts (Y=1.755) carried that offset
                //    forever; this now rounds Y to the universal grid
                //    so parts converge to clean grid lines as you drag.
                //
                // 3. No snap (snap_enabled = false) — use the raw
                //    target_pos unchanged.
                let final_target_pos = if editor_settings.snap_enabled {
                    let grab_world = target_pos + grab_offset_world;
                    let snapped_grab = if let Some((tgt_rot, tgt_center, surf_n)) = surface_frame {
                        math_snap_to_grid_in_frame(
                            grab_world,
                            tgt_center,
                            tgt_rot,
                            surf_n,
                            editor_settings.snap_size,
                        )
                    } else {
                        math_snap_to_grid(grab_world, editor_settings.snap_size)
                    };
                    let mut center = snapped_grab - grab_offset_world;
                    // **Anti-clipping guard.** If we have a surface frame,
                    // the grid-snap may have rounded the grabbed-point's
                    // Y onto a grid line that doesn't sit on the surface
                    // plane — yanking the part's center along the normal
                    // and burying part of it into the geometry. Restore
                    // the unsnapped center's normal-axis component so
                    // the contact face stays flush. Tangent axes still
                    // snap, which gives the "click to grid along the
                    // surface" feel without the clip-through.
                    if let Some((_, _, surf_n)) = surface_frame {
                        let n = surf_n.normalize_or_zero();
                        if n.length_squared() > 0.5 {
                            // Project both vectors onto the normal and
                            // swap in the unsnapped component.
                            let snapped_along_n = center.dot(n);
                            let original_along_n = target_pos.dot(n);
                            center += n * (original_along_n - snapped_along_n);
                        }
                    }
                    center
                } else {
                    target_pos
                };

                state.last_target_position = final_target_pos;
                
                // 3. Apply Transformations
                // Move group rigidly relative to leader
                // pivot = initial_leader_pos
                let pivot = initial_leader_pos;
                
                // Collect selected entities set for hierarchy check
                let selected_entities: std::collections::HashSet<Entity> = selected_query.iter().map(|(e, ..)| e).collect();

                // Update selected entities
                for (entity, mut transform, _, _, _, basepart_opt) in selected_query.iter_mut() {
                    // Check if any ancestor is also selected
                    // If so, skip this entity (it will be moved by its parent)
                    let mut is_descendant = false;
                    let mut current = entity;
                    while let Ok(child_of) = parent_query.get(current) {
                        let parent_entity = child_of.parent();
                        if selected_entities.contains(&parent_entity) {
                            is_descendant = true;
                            break;
                        }
                        current = parent_entity;
                    }
                    if is_descendant { continue; }

                    if let (Some(initial_pos), Some(initial_rot)) = (state.initial_positions.get(&entity), state.initial_rotations.get(&entity)) {
                        
                        // New Position = Pivot + RotationDelta * (InitialPos - Pivot) + TranslationDelta
                        // (Rotate around pivot, then translate to new location)
                        // Actually:
                        // 1. Relative pos from pivot: rel = initial - pivot
                        // 2. Rotate rel: rel_rot = rot_delta * rel
                        // 3. New pos = final_target_pos + rel_rot (since final_target_pos IS the new pivot location)
                        
                        let relative_pos = *initial_pos - pivot;
                        let rotated_relative_pos = rotation_delta * relative_pos;
                        let raw_pos = final_target_pos + rotated_relative_pos;
                        // Clamp NaN + cap at MAX_WORLD_EXTENT (5000)
                        // before writing — drag-into-the-sky shouldn't
                        // be able to teleport the part to ±∞ where
                        // Avian's AABB math overflows into NaN.
                        let new_pos = crate::space::instance_loader::safe_translation(
                            raw_pos, *initial_pos,
                        );

                        let new_rot = rotation_delta * *initial_rot;

                        transform.translation = new_pos;
                        transform.rotation = new_rot;

                        // Update BasePart
                        if let Some(mut bp) = basepart_opt {
                            bp.cframe.translation = new_pos;
                            bp.cframe.rotation = new_rot;
                        }
                    }
                }
            }
        }
    } else if mouse.just_released(MouseButton::Left) {
        // Record undo action if we actually dragged (threshold exceeded)
        if state.drag_started && !state.initial_positions.is_empty() {
            // Collect old and new transforms for undo
            let mut old_transforms: Vec<(u64, [f32; 3], [f32; 4])> = Vec::new();
            let mut new_transforms: Vec<(u64, [f32; 3], [f32; 4])> = Vec::new();
            
            for (entity, transform, _, _, _, _) in selected_query.iter() {
                if let Some(initial_pos) = state.initial_positions.get(&entity) {
                    if let Some(initial_rot) = state.initial_rotations.get(&entity) {
                        // Only record if position or rotation actually changed
                        let pos_changed = (*initial_pos - transform.translation).length() > 0.001;
                        let rot_changed = initial_rot.angle_between(transform.rotation) > 0.001;
                        
                        if pos_changed || rot_changed {
                            old_transforms.push((
                                entity.to_bits(),
                                initial_pos.to_array(),
                                initial_rot.to_array(),
                            ));
                            new_transforms.push((
                                entity.to_bits(),
                                transform.translation.to_array(),
                                transform.rotation.to_array(),
                            ));
                        }
                    }
                }
            }
            
            // Push to undo stack if there were actual changes
            if !old_transforms.is_empty() {
                undo_stack.push(crate::undo::Action::TransformEntities {
                    old_transforms,
                    new_transforms,
                });
            }

            // Write updated transforms back to TOML (file-system-first persistence)
            for (entity, transform, _, _, _, _) in selected_query.iter() {
                if let Ok(inst_file) = instance_files.get(entity) {
                    if let Ok(mut def) = crate::space::instance_loader::load_instance_definition(&inst_file.toml_path) {
                        def.transform.position = [transform.translation.x, transform.translation.y, transform.translation.z];
                        def.transform.rotation = [transform.rotation.x, transform.rotation.y, transform.rotation.z, transform.rotation.w];
                        def.metadata.last_modified = chrono::Utc::now().to_rfc3339();
                        let _ = crate::space::instance_loader::write_instance_definition(&inst_file.toml_path, &def);
                    }
                }
            }
        }

        state.dragging = false;
        state.drag_started = false;
        state.dragged_entity = None;
        state.initial_positions.clear();
        state.initial_rotations.clear();
        // Reset group bounds
        state.group_center = Vec3::ZERO;
        state.group_bounds_min = Vec3::ZERO;
        state.group_bounds_max = Vec3::ZERO;
        state.group_size = Vec3::ONE;
        // Reset smoothing state
        state.last_target_position = Vec3::ZERO;
        state.last_surface_normal = Vec3::Y;
        state.last_hit_entity = None;
    }
    
    // Ctrl+R / Ctrl+T rotate + tilt on the current selection. Works
    // for single AND multi-selection; the multi-select path rotates the
    // whole group around its averaged centroid so the cluster pivots as
    // one rigid body instead of each part spinning in place. Previously
    // this was gated behind `state.dragging`, which meant the user had
    // to hold a drag to make rotation fire — painful for "select then
    // rotate" workflows. The outer `ui_focus` check above already
    // blocks firing when a text field has focus, so we don't have to
    // re-guard typing here.
    //
    // We accept raw `R` / `T` AND Ctrl+R / Ctrl+T: the keybindings
    // dispatcher fires `Action::RotateY90` on Ctrl+R but has no match
    // arm, so this handler is the only responder. Matching on raw R/T
    // in addition to the Ctrl form preserves the legacy in-drag
    // behaviour without forcing modifier-held state.
    let rotate_pressed = keys.just_pressed(KeyCode::KeyR);
    let tilt_pressed = keys.just_pressed(KeyCode::KeyT);
    if rotate_pressed || tilt_pressed {
        // Group centroid — average of every selected part's current
        // translation. For single-select this collapses to the part's
        // own position, so the pivot is always sensible.
        let mut group_center = Vec3::ZERO;
        let mut count = 0;
        for (_, transform, _, _, _, _) in selected_query.iter() {
            group_center += transform.translation;
            count += 1;
        }
        if count == 0 {
            return;
        }
        group_center /= count as f32;

        // Skip children of already-selected entities: rotating a
        // Model's root part would double-transform its mesh children
        // if the hierarchy is intact. The descendant filter below
        // walks `ChildOf` until it hits a selected ancestor; if one
        // exists the child gets skipped so only the top-of-selection
        // entities move.
        let selected_entities: std::collections::HashSet<Entity> =
            selected_query.iter().map(|(e, ..)| e).collect();

        if rotate_pressed {
            let rotation = Quat::from_rotation_y(90.0_f32.to_radians());
            for (entity, mut transform, _, _, _, _) in selected_query.iter_mut() {
                let mut is_descendant = false;
                let mut current = entity;
                while let Ok(child_of) = parent_query.get(current) {
                    let parent_entity = child_of.parent();
                    if selected_entities.contains(&parent_entity) {
                        is_descendant = true;
                        break;
                    }
                    current = parent_entity;
                }
                if is_descendant { continue; }
                let relative_pos = transform.translation - group_center;
                transform.translation = group_center + rotation * relative_pos;
                transform.rotate_y(90.0_f32.to_radians());
                // Keep drag caches in sync so if the user IS mid-drag
                // the next frame's drag-follow doesn't snap the
                // rotation back.
                state.initial_positions.insert(entity, transform.translation);
                state.initial_rotations.insert(entity, transform.rotation);
            }
        }

        if tilt_pressed {
            let tilt = Quat::from_rotation_z(90.0_f32.to_radians());
            for (entity, mut transform, _, _, _, _) in selected_query.iter_mut() {
                let mut is_descendant = false;
                let mut current = entity;
                while let Ok(child_of) = parent_query.get(current) {
                    let parent_entity = child_of.parent();
                    if selected_entities.contains(&parent_entity) {
                        is_descendant = true;
                        break;
                    }
                    current = parent_entity;
                }
                if is_descendant { continue; }
                let relative_pos = transform.translation - group_center;
                transform.translation = group_center + tilt * relative_pos;
                transform.rotate_z(90.0_f32.to_radians());
                state.initial_positions.insert(entity, transform.translation);
                state.initial_rotations.insert(entity, transform.rotation);
            }
        }
    }
    
    // `+` / `-` nudging lives in `keybindings.rs::nudge_selection_system`
    // exclusively — that handler uses a proper first-press + auto-repeat
    // timer. A duplicate `pressed()`-based handler used to live here and
    // fired once per frame while the key was held, producing a double-
    // or N-unit jump for every tap. Removed.
}

/// Ray-OBB intersection returning distance (for paste raycasting)
/// Works on ALL parts regardless of can_collide setting
pub fn ray_intersects_part_rotated_distance(ray: &Ray3d, position: Vec3, rotation: Quat, size: Vec3) -> Option<f32> {
    ray_obb_intersection(ray.origin, *ray.direction, position, size * 0.5, rotation)
}

// ============================================================================
// Box Selection Systems
// ============================================================================

/// System to handle box selection (drag to select multiple entities)
fn handle_box_selection(
    mut box_state: ResMut<BoxSelectionState>,
    select_state: Option<Res<SelectToolState>>,
    studio_state: Option<Res<StudioState>>,
    ui_focus: Option<Res<SlintUIFocus>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    selection_manager: Option<Res<BevySelectionManager>>,
    // `ViewportBounds` holds the Slint-hosted 3D viewport's offset inside
    // the window (physical pixels). Camera renders with a non-default
    // `Viewport` so `camera.world_to_viewport()` returns VIEWPORT-local
    // coords, while `window.cursor_position()` returns WINDOW-local coords.
    // We read the bounds here to bridge the two spaces inside the box
    // containment test — mixing them silently rejected every marquee
    // selection on displays where the left UI panel had non-zero width.
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
    // Query entities with PartEntity OR Instance (supports both legacy and modern)
    parts_query: Query<(Entity, &GlobalTransform, Option<&BasePart>, Option<&PartEntity>, Option<&Instance>), Or<(With<PartEntity>, With<Instance>)>>,
    selected_query: Query<Entity, With<Selected>>,
) {
    let Some(selection_manager) = selection_manager else { return };
    let Some(studio_state) = studio_state else { return };
    let Some(select_state) = select_state else { return };
    // Active in Select, Move, Scale, and Rotate tool modes
    // Box selection should work in all transformation tools
    let box_select_enabled = matches!(
        studio_state.current_tool,
        Tool::Select | Tool::Move | Tool::Scale | Tool::Rotate
    );
    
    if !box_select_enabled {
        if box_state.active || box_state.pending {
            box_state.active = false;
            box_state.pending = false;
        }
        return;
    }
    
    // Block input when Slint UI has focus (mouse is over UI panels)
    if let Some(ui_focus) = ui_focus {
        if ui_focus.has_focus {
            return;
        }
    }
    
    let Ok(window) = windows.single() else { return; };
    let Some(cursor_pos) = window.cursor_position() else { return; };
    let Some((camera, camera_transform)) = cameras.iter().find(|(c, _)| c.order == 0) else { return; };
    
    // Check if Shift is held for additive selection
    let shift_held = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    
    if mouse.just_pressed(MouseButton::Left) {
        // Reset any stale state from previous interactions
        box_state.active = false;
        box_state.pending = false;

        // Don't start box select if we're already dragging a part
        if select_state.dragging {
            info!("📦 box-select skipped: mid-drag of a part");
            return;
        }

        // Check if clicking on a SELECTABLE part (not locked)
        let ray = camera.viewport_to_world(camera_transform, cursor_pos).ok();
        let clicking_on_part = ray.map(|r| {
            parts_query.iter().any(|(_, transform, basepart, _, _)| {
                // Skip locked parts - clicking on them should start box selection
                if let Some(bp) = basepart {
                    if bp.locked {
                        return false;
                    }
                }
                let t = transform.compute_transform();
                let size = basepart.map(|bp| bp.size).unwrap_or(Vec3::ONE);
                ray_intersects_part(r.origin, *r.direction, &t, size).is_some()
            })
        }).unwrap_or(false);

        info!(
            "📦 box-select press: cursor={:?} clicking_on_part={} shift={} parts_total={}",
            cursor_pos,
            clicking_on_part,
            shift_held,
            parts_query.iter().count(),
        );

        if !clicking_on_part {
            // Start potential box selection - mark as pending
            box_state.pending = true;
            box_state.start_pos = cursor_pos;
            box_state.current_pos = cursor_pos;
            box_state.additive = shift_held;
            info!("📦 Box selection PENDING at {:?}", cursor_pos);
            
            // Store current selection for additive mode
            if shift_held {
                box_state.previous_selection = selected_query.iter().collect();
            } else {
                // Clear selection immediately when clicking on empty space (non-additive)
                // This provides instant feedback - box selection will re-select if dragged
                let sm = selection_manager.0.write();
                sm.clear();
                box_state.previous_selection.clear();
            }
        }
    } else if mouse.pressed(MouseButton::Left) && box_state.pending && !select_state.dragging {
        // Only update box selection if we have a pending selection from THIS click
        let drag_distance = (cursor_pos - box_state.start_pos).length();
        
        if drag_distance > BOX_SELECT_THRESHOLD {
            let transitioned = !box_state.active;
            if transitioned {
                info!(
                    "📦 Box selection ACTIVE - drag {} → {} (distance={:.1}px, threshold={})",
                    format!("{:?}", box_state.start_pos),
                    format!("{:?}", cursor_pos),
                    drag_distance,
                    BOX_SELECT_THRESHOLD,
                );
            }
            box_state.active = true;
            box_state.current_pos = cursor_pos;

            // Bridge the two coordinate spaces:
            //   - `box_state.start_pos` / `cursor_pos` are WINDOW-logical
            //     (from `window.cursor_position()`).
            //   - `camera.world_to_viewport()` returns VIEWPORT-logical
            //     because the Bevy `Camera` has a non-default `Viewport`
            //     (the Slint UI hosts the 3D view in an offset rectangle).
            // Subtracting the viewport's logical top-left from the cursor
            // positions yields viewport-local box bounds that line up with
            // the projected entity centers. The legacy code compared the
            // two spaces directly, so any display with a left UI panel
            // silently rejected every marquee selection.
            let (vp_ox, vp_oy) = viewport_bounds
                .as_deref()
                .map(|vb| {
                    let s = window.scale_factor() as f32;
                    (vb.x / s.max(0.0001), vb.y / s.max(0.0001))
                })
                .unwrap_or((0.0, 0.0));

            let sx = box_state.start_pos.x - vp_ox;
            let ex = cursor_pos.x - vp_ox;
            let sy = box_state.start_pos.y - vp_oy;
            let ey = cursor_pos.y - vp_oy;
            let min_x = sx.min(ex);
            let max_x = sx.max(ex);
            let min_y = sy.min(ey);
            let max_y = sy.max(ey);

            if transitioned {
                info!(
                    "📦 box-select rect (viewport-local): x=[{:.1}..{:.1}] y=[{:.1}..{:.1}] vp_offset=({:.1},{:.1})",
                    min_x, max_x, min_y, max_y, vp_ox, vp_oy,
                );
            }

            // Find all part_ids within the box
            let mut part_ids_in_box: Vec<String> = Vec::new();
            let mut projected_count = 0;
            let mut overlap_count = 0;

            for (entity, transform, basepart, part_entity, instance) in parts_query.iter() {
                // Skip locked parts - they shouldn't be selectable via box selection
                if let Some(bp) = basepart {
                    if bp.locked {
                        continue;
                    }
                }

                // Project the entity's full OBB to screen-space and test
                // RECTANGLE INTERSECTION rather than center containment.
                // A long horizontal bar (5×1×1) with its center outside
                // the marquee but most of its body inside should still
                // select — matches Blender / Maya convention.
                let t = transform.compute_transform();
                let size = basepart.map(|bp| bp.size).unwrap_or(t.scale);
                let half = size * 0.5;
                let corners = [
                    Vec3::new(-half.x, -half.y, -half.z),
                    Vec3::new( half.x, -half.y, -half.z),
                    Vec3::new(-half.x,  half.y, -half.z),
                    Vec3::new( half.x,  half.y, -half.z),
                    Vec3::new(-half.x, -half.y,  half.z),
                    Vec3::new( half.x, -half.y,  half.z),
                    Vec3::new(-half.x,  half.y,  half.z),
                    Vec3::new( half.x,  half.y,  half.z),
                ];
                let mut ent_min_x = f32::MAX;
                let mut ent_max_x = f32::MIN;
                let mut ent_min_y = f32::MAX;
                let mut ent_max_y = f32::MIN;
                let mut projected_any = false;
                for c in corners {
                    let world_corner = t.translation + t.rotation * c;
                    if let Ok(sp) = camera.world_to_viewport(camera_transform, world_corner) {
                        // Bevy 0.18 `world_to_viewport` returns PHYSICAL pixels
                        // when the camera has an explicit `Viewport` (which is
                        // the Slint-hosted case). Scale back to LOGICAL to
                        // match the rect (built from logical-pixel cursor +
                        // logical-pixel viewport offset).
                        let s = window.scale_factor() as f32;
                        let s = if s > 0.0 { s } else { 1.0 };
                        let sp_logical = Vec2::new(sp.x / s, sp.y / s);
                        ent_min_x = ent_min_x.min(sp_logical.x);
                        ent_max_x = ent_max_x.max(sp_logical.x);
                        ent_min_y = ent_min_y.min(sp_logical.y);
                        ent_max_y = ent_max_y.max(sp_logical.y);
                        projected_any = true;
                    }
                }
                if !projected_any { continue; }
                projected_count += 1;

                // Standard AABB-AABB overlap test in viewport space.
                let overlaps = ent_max_x >= min_x
                    && ent_min_x <= max_x
                    && ent_max_y >= min_y
                    && ent_min_y <= max_y;
                if overlaps {
                    overlap_count += 1;
                    // Get part_id from PartEntity or Instance
                    let part_id = if let Some(pe) = part_entity {
                        if !pe.part_id.is_empty() {
                            pe.part_id.clone()
                        } else if instance.is_some() {
                            format!("{}v{}", entity.index(), entity.generation())
                        } else {
                            continue;
                        }
                    } else if instance.is_some() {
                        format!("{}v{}", entity.index(), entity.generation())
                    } else {
                        continue;
                    };
                    part_ids_in_box.push(part_id);
                }
            }

            if transitioned {
                info!(
                    "📦 box-select scan: projected={} overlaps={} selected_ids={}",
                    projected_count,
                    overlap_count,
                    part_ids_in_box.len(),
                );
            }

            // Update selection using part_ids
            let sm = selection_manager.0.write();
            
            if box_state.additive {
                // Keep previous selection and add new ones
                sm.clear();
                // Re-add previous selection
                for entity in &box_state.previous_selection {
                    // Get part_id from entity by querying
                    if let Ok((_, _, _, pe, inst)) = parts_query.get(*entity) {
                        let part_id = if let Some(p) = pe {
                            if !p.part_id.is_empty() { p.part_id.clone() }
                            else if inst.is_some() { format!("{}v{}", entity.index(), entity.generation()) }
                            else { continue; }
                        } else if inst.is_some() {
                            format!("{}v{}", entity.index(), entity.generation())
                        } else {
                            continue;
                        };
                        sm.select(part_id);
                    }
                }
                for part_id in &part_ids_in_box {
                    sm.select(part_id.clone());
                }
            } else {
                // Replace selection with box contents
                sm.clear();
                for part_id in &part_ids_in_box {
                    sm.select(part_id.clone());
                }
            }
        }
    } else if mouse.just_released(MouseButton::Left) {
        // If we had a pending box selection that never became active (single click on empty space)
        // Clear the selection (unless shift was held for additive mode)
        if box_state.pending && !box_state.active && !box_state.additive {
            let sm = selection_manager.0.write();
            sm.clear();
            info!("Cleared selection - clicked on empty space");
        }
        
        // Reset all box selection state on mouse release
        box_state.active = false;
        box_state.pending = false;
        box_state.previous_selection.clear();
    }
}

/// System to render the box selection rectangle using Bevy gizmos
/// Sync box selection state to Slint overlay properties.
/// The rectangle is rendered by Slint inside the viewport-sizer, on top of
/// the 3D scene but below studio panels.
fn render_box_selection(
    box_state: Res<BoxSelectionState>,
    slint_context: Option<NonSend<crate::ui::slint_ui::SlintUiState>>,
) {
    let Some(ctx) = slint_context else { return };
    let ui = &ctx.window;

    if !box_state.active {
        ui.set_box_select_visible(false);
        return;
    }

    // cursor_position() returns logical pixels; viewport-sizer position is also
    // logical (set via get_viewport_x/y * scale in sync_bevy_to_slint).
    // Subtract viewport offset to get coordinates relative to viewport-sizer.
    let vp_x = ui.get_viewport_x();
    let vp_y = ui.get_viewport_y();

    let x1 = box_state.start_pos.x - vp_x;
    let y1 = box_state.start_pos.y - vp_y;
    let x2 = box_state.current_pos.x - vp_x;
    let y2 = box_state.current_pos.y - vp_y;

    let min_x = x1.min(x2);
    let min_y = y1.min(y2);
    let w = (x2 - x1).abs();
    let h = (y2 - y1).abs();

    ui.set_box_select_visible(true);
    ui.set_box_select_x(min_x);
    ui.set_box_select_y(min_y);
    ui.set_box_select_w(w);
    ui.set_box_select_h(h);
}
