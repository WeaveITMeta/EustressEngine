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
use crate::ui::{StudioState, Tool, SlintUIFocus};
// IMPORTANT: use the `rendering::BevySelectionManager` resource, NOT the
// `ui::BevySelectionManager` one. They are distinct Resource TYPES wrapping the
// SAME `Arc<RwLock<SelectionManager>>` (cloned to both plugins in main.rs), but
// only the rendering one is actually inserted at runtime — by `PartRenderingPlugin`
// (added in main.rs). The ui one is inserted solely by the LEGACY, NOT-added
// `StudioUiPlugin`, so `Option<Res<ui::BevySelectionManager>>` was always `None`
// and `handle_box_selection` early-returned on its first line every frame
// (box-select "didn't work at all"). part_selection.rs already uses the rendering
// one — match it.
use crate::rendering::BevySelectionManager;
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
    /// Camera-relative free-space drag: distance from the camera to the
    /// grabbed entity, captured when the drag starts and adjustable by
    /// mouse wheel while dragging (see `handle_drag_distance_wheel`).
    /// Only used by the empty-space fallback (no surface under cursor) —
    /// surface-follow dragging is unaffected. Recomputing the drag plane
    /// from this distance and the CURRENT camera transform every frame
    /// (not the grab-time transform) is what lets the dragged part follow
    /// the camera through WASD movement and look-around mid-drag.
    pub drag_camera_distance: f32,
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
            drag_camera_distance: 10.0,
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
    /// The marquee rectangle in VIEWPORT-LOCAL LOGICAL pixels, computed by
    /// `handle_box_selection` from `ViewportBounds` — the SAME rect the
    /// selection scan uses. `render_box_selection` draws exactly this so the
    /// visible box matches the selection region (WYSIWYG). Without sharing it,
    /// the renderer recomputed the rect from a DIFFERENT viewport-offset source
    /// (`get_viewport_x`), so the drawn box was shifted from what it selected.
    pub rect_min: Vec2,
    pub rect_size: Vec2,
}

/// Plugin for the select tool drag functionality
pub struct SelectToolPlugin;

impl Plugin for SelectToolPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<SelectToolState>()
            .init_resource::<BoxSelectionState>()
            .add_systems(Update, (
                handle_drag_distance_wheel
                    .before(handle_select_drag),
                handle_select_drag
                    .after(crate::ui::slint_ui::update_slint_ui_focus)
                    .after(handle_drag_distance_wheel),
                handle_box_selection
                    .after(handle_select_drag),
                debug_drag_gizmos.after(handle_box_selection),
            ))
            // render_box_selection uses NonSend<SlintUiState> — must run separately
            // to avoid blocking the chain on main thread exclusivity
            .add_systems(Update, render_box_selection.after(handle_box_selection));
    }
}

/// While a node is being dragged through empty space (see the camera-relative
/// fallback in `handle_select_drag`), the mouse wheel changes the LEASH
/// LENGTH instead of zooming the camera — same pattern as
/// `part_selection::hover_resize_system` repurposing the wheel for the
/// Ctrl+Shift+Alt resize chord, just gated on drag state instead of a key
/// chord. `eustress_camera_controls` separately zeroes its own scroll delta
/// while `state.dragging` is true, so the same notch never ALSO zooms the
/// camera.
///
/// Direction, as specified: scrolling down brings the part closer, scrolling
/// up sends it farther — the opposite sign relationship from the camera's
/// own fly-zoom (where positive/scroll-up moves the camera forward/closer).
/// That inversion is deliberate, not a bug — flag it if it feels backwards.
fn handle_drag_distance_wheel(
    mut ev_wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut state: ResMut<SelectToolState>,
) {
    use bevy::input::mouse::MouseScrollUnit;

    let mut scroll = 0.0_f32;
    for ev in ev_wheel.read() {
        scroll += if ev.unit == MouseScrollUnit::Line { ev.y } else { ev.y * 0.1 };
    }
    if scroll == 0.0 {
        return;
    }
    if !(state.dragging && state.drag_started) {
        return;
    }

    // Same exponential shape as the camera's fly-zoom (ZOOM_STEP = 0.9) for
    // a consistent feel, sign flipped: scroll up (+) grows the leash,
    // scroll down (-) shrinks it.
    const LEASH_STEP: f32 = 0.9;
    let multiplier = LEASH_STEP.powf(-scroll);
    state.drag_camera_distance = (state.drag_camera_distance * multiplier).clamp(0.5, 5000.0);
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
    _spatial_query: SpatialQuery,
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
            // Closest ray-OBB hit among the selected entity AND its
            // descendants. The descendant test is what lets you grab a
            // Model: after the Workspace reorg, a Model sits at the world
            // origin with no BasePart, so its own OBB is a 1×1×1 box at
            // [0,0,0] — pressing on its visible child parts missed it and
            // the drag never engaged ("the part won't move"). Walking the
            // children fixes that; the grab pivot below encodes the hit
            // point relative to the selected entity so surface-follow keeps
            // the grabbed child under the cursor.
            let mut grab_hit: Option<(f32, Vec3)> = crate::math_utils::ray_obb_intersection(
                ray.origin, *ray.direction, t.translation, size * 0.5, t.rotation,
            ).map(|th| (th, ray.origin + *ray.direction * th));
            {
                let mut stack: Vec<Entity> = Vec::new();
                if let Ok(kids) = children_query.get(entity) { stack.extend(kids.iter()); }
                while let Some(d) = stack.pop() {
                    if let Ok((_, d_gt, _, _, _, d_bp)) = all_parts_query.get(d) {
                        let dt = d_gt.compute_transform();
                        let dsize = d_bp.map(|bp| bp.size).unwrap_or(dt.scale);
                        if let Some(th) = crate::math_utils::ray_obb_intersection(
                            ray.origin, *ray.direction, dt.translation, dsize * 0.5, dt.rotation,
                        ) {
                            if grab_hit.map_or(true, |(b, _)| th < b) {
                                grab_hit = Some((th, ray.origin + *ray.direction * th));
                            }
                        }
                    }
                    if let Ok(kids) = children_query.get(d) { stack.extend(kids.iter()); }
                }
            }
            if let Some((_t_hit, grab_world)) = grab_hit {
                // Convert to the selected entity's local frame so the offset
                // rotates with it and stays attached to the same physical
                // point.
                let grab_offset_local = t.rotation.inverse() * (grab_world - t.translation);
                state.grab_offset_local = grab_offset_local;
                state.dragging = true;
                state.drag_started = false; // Not started until threshold exceeded
                state.dragged_entity = Some(entity);
                state.initial_position = transform.translation;
                state.initial_cursor_pos = cursor_pos; // Store initial cursor position
                state.drag_offset = transform.translation - ray.origin;
                // Lock the camera-relative drag distance at grab time — the
                // empty-space fallback re-derives its plane from this distance
                // and the LIVE camera transform every frame (see below), not
                // from this grab-time position, so WASD/look mid-drag works.
                state.drag_camera_distance = (transform.translation - camera_transform.translation()).length().max(0.1);
                
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
                // Exclude the FULL descendant tree of every selected entity
                // (not just direct children) so a dragged Model never snaps
                // its surface to its own child parts — and so selection
                // adornments/wireframes don't interfere with the raycast.
                {
                    let mut stack: Vec<Entity> = excluded_entities.clone();
                    while let Some(p) = stack.pop() {
                        if let Ok(children) = children_query.get(p) {
                            for k in children.iter() {
                                excluded_entities.push(k);
                                stack.push(k);
                            }
                        }
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
                
                // 1. Find Surface from VISIBLE geometry only (the part-size
                //    OBB — "what you see is the snap surface"). The physics
                //    collider is intentionally NOT consulted, so collider
                //    sizing can never corrupt where a dragged part lands.
                //    `find_surface_with_normal` uses ray_obb_entry, which skips
                //    any box the camera is inside (no teleport-to-camera).
                let surface_hit = math_find_surface_with_normal(&ray, &all_parts_query, &excluded_entities)
                    .map(|(pt, norm, ent)| (pt, norm, Some(ent)));

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
                    // Cursor is over the skybox / nothing to land on.
                    //
                    // Camera-relative free-space drag: instead of a
                    // world-locked horizontal plane (the old behavior —
                    // fine for building on a baseplate, wrong for floating
                    // mind-map nodes with nothing underneath them), hold
                    // the part at a FIXED DISTANCE from the camera, like
                    // leading it on a rope. The plane is rebuilt from the
                    // camera's transform THIS FRAME (not the grab-time
                    // transform), so WASD movement and look-around during
                    // the drag naturally carry the part with you — it
                    // falls out of recomputing every frame, no special
                    // casing needed.
                    state.debug_hit_point = None;
                    state.debug_hit_normal = None;

                    let cam_pos = camera_transform.translation();
                    let cam_fwd = *camera_transform.forward();
                    let plane_point = cam_pos + cam_fwd * state.drag_camera_distance;

                    if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, plane_point, cam_fwd) {
                        let cursor_world = ray.origin + *ray.direction * t;
                        // Glue the grabbed point to the cursor on this
                        // camera-facing plane — full 3D offset, unlike the
                        // old horizontal plane which only tracked X/Z.
                        cursor_world - grab_offset_world
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
    // Transform-tool states: a grabbed gizmo handle (dragged_axis set) must
    // pre-empt the marquee so it doesn't fight the gizmo.
    move_state: Option<Res<crate::move_tool::MoveToolState>>,
    scale_state: Option<Res<crate::scale_tool::ScaleToolState>>,
    rotate_state: Option<Res<crate::rotate_tool::RotateToolState>>,
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

        // MARQUEE START RULE (user choice 2026-05-24 "works on any drag"): start
        // a pending marquee on ANY left-press, EXCEPT:
        //   * a press that grabbed an already-SELECTED part — `handle_select_drag`
        //     runs first and sets `select_state.dragging`, caught by the
        //     `if select_state.dragging { return }` guard above (that's the move
        //     gesture); and
        //   * a press that grabbed a gizmo handle (`dragged_axis` set on the
        //     move/scale/rotate tool).
        // Everything else — empty space, the grass, the floor, an UNSELECTED part
        // (locked or not) — begins the rubber-band. We do NOT clear the selection
        // on press: a no-drag click is owned by `part_selection` (it selects the
        // clicked part or deselects on empty), and a real drag REPLACES the
        // selection in the active branch below. (Locked parts are still EXCLUDED
        // from the marquee's selection results — see the scan loop.)
        let handle_grabbed = move_state.as_ref().and_then(|s| s.dragged_axis).is_some()
            || scale_state.as_ref().and_then(|s| s.dragged_axis).is_some()
            || rotate_state.as_ref().and_then(|s| s.dragged_axis).is_some();

        info!(
            "📦 box-select press: cursor={:?} handle={} shift={} parts_total={}",
            cursor_pos,
            handle_grabbed,
            shift_held,
            parts_query.iter().count(),
        );

        if !handle_grabbed {
            // Start a pending marquee (drag past threshold turns it active).
            box_state.pending = true;
            box_state.start_pos = cursor_pos;
            box_state.current_pos = cursor_pos;
            box_state.additive = shift_held;
            info!("📦 Box selection PENDING at {:?}", cursor_pos);

            // For additive (Shift) mode, remember the pre-drag selection so the
            // marquee adds to it. Non-additive: do NOT clear here — let
            // part_selection own a plain click; the active branch replaces the
            // selection once an actual drag begins.
            box_state.previous_selection = if shift_held {
                selected_query.iter().collect()
            } else {
                Vec::new()
            };
        }
    } else if mouse.pressed(MouseButton::Left) && box_state.pending && !select_state.dragging {
        // If a transform handle got grabbed after the press (its dragged_axis
        // is set a frame later), abort the pending marquee so it doesn't fight
        // the gizmo drag.
        let handle_grabbed = move_state.as_ref().and_then(|s| s.dragged_axis).is_some()
            || scale_state.as_ref().and_then(|s| s.dragged_axis).is_some()
            || rotate_state.as_ref().and_then(|s| s.dragged_axis).is_some();
        if handle_grabbed {
            box_state.pending = false;
            box_state.active = false;
            return;
        }
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

            // COORDINATE SPACES (fixed 2026-05-24):
            //   - The editor camera renders FULL-WINDOW (that's why single-click
            //     selection works by feeding the raw WINDOW cursor to
            //     `viewport_to_world` in part_selection.rs). So
            //     `camera.world_to_viewport()` below also returns WINDOW pixels.
            //   - `ViewportBounds` is ONLY the Slint sizer's VISIBLE region (used
            //     to gate clicks + to offset the DRAWN box), NOT the camera's
            //     viewport. It must therefore NOT be subtracted from the SELECTION
            //     rect. The old code subtracted it, so the box was compared against
            //     parts ~panel-width (~280px) away — "wildly inaccurate, selects
            //     far-off parts".
            let (vp_ox, vp_oy) = viewport_bounds
                .as_deref()
                .map(|vb| {
                    let s = window.scale_factor() as f32;
                    (vb.x / s.max(0.0001), vb.y / s.max(0.0001))
                })
                .unwrap_or((0.0, 0.0));

            // SELECTION rect — WINDOW-logical, matching `world_to_viewport`.
            let win_min_x = box_state.start_pos.x.min(cursor_pos.x);
            let win_max_x = box_state.start_pos.x.max(cursor_pos.x);
            let win_min_y = box_state.start_pos.y.min(cursor_pos.y);
            let win_max_y = box_state.start_pos.y.max(cursor_pos.y);

            // RENDER rect (stored) — viewport-sizer-local: the Slint Rectangle
            // lives INSIDE the sizer (offset by vp_ox/vp_oy), so subtract that
            // offset here so the drawn box sits exactly under the cursor.
            box_state.rect_min = Vec2::new(win_min_x - vp_ox, win_min_y - vp_oy);
            box_state.rect_size = Vec2::new(win_max_x - win_min_x, win_max_y - win_min_y);

            if transitioned {
                info!(
                    "📦 box-select rect: window x=[{:.1}..{:.1}] y=[{:.1}..{:.1}] sizer_offset=({:.1},{:.1})",
                    win_min_x, win_max_x, win_min_y, win_max_y, vp_ox, vp_oy,
                );
            }

            // Find all part_ids within the box
            let mut part_ids_in_box: Vec<String> = Vec::new();
            let mut projected_count = 0;
            let mut overlap_count = 0;

            for (entity, transform, basepart, part_entity, instance) in parts_query.iter() {
                // Only real scene PARTS are box-selectable. Skip anything without a
                // BasePart — cameras (incl. the AI camera), Models/Folders, and GUI
                // entities all carry an `Instance` so they're in the query, but they
                // are NOT parts and must never be marquee-selected (the user hit the
                // camera being selected). Locked parts are also skipped.
                let Some(bp) = basepart else { continue; };
                if bp.locked { continue; }

                // Project the entity's full OBB to screen-space and test
                // RECTANGLE INTERSECTION rather than center containment.
                // A long horizontal bar (5×1×1) with its center outside
                // the marquee but most of its body inside should still
                // select — matches Blender / Maya convention.
                let t = transform.compute_transform();
                let size = bp.size;
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

                // Standard AABB-AABB overlap test in WINDOW space (the projected
                // corners and the rect are both window-logical now).
                let overlaps = ent_max_x >= win_min_x
                    && ent_min_x <= win_max_x
                    && ent_max_y >= win_min_y
                    && ent_min_y <= win_max_y;
                if overlaps {
                    overlap_count += 1;
                    // The selection id MUST be the entity index/generation — that
                    // is exactly what `selection_sync::get_part_id` matches on
                    // (it IGNORES PartEntity.part_id). Using PartEntity.part_id
                    // here put the wrong key in the selection set for OLD parts
                    // (which carry a non-empty part_id), so the sync never
                    // highlighted them — "box-select works for new parts but not
                    // old parts". `part_entity`/`instance` are unused now.
                    let _ = (part_entity, instance);
                    part_ids_in_box.push(format!("{}v{}", entity.index(), entity.generation()));
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

            // Update selection using part_ids.
            //
            // CRITICAL: use `add_to_selection`, NOT `select`. `select()` is
            // single-select — it CLEARS the set and pushes one id on every
            // call (correct for a single click). Looping `select()` here left
            // only the LAST part selected, so the marquee appeared to grab one
            // part instead of every part inside the box. `add_to_selection`
            // appends (and de-dupes) without clearing, so the whole box-set
            // accumulates. We clear ONCE up-front, then accumulate.
            let sm = selection_manager.0.write();

            if box_state.additive {
                // Keep previous selection and add new ones. Same id contract:
                // entity index/generation (what selection_sync matches on).
                sm.clear();
                for entity in &box_state.previous_selection {
                    sm.add_to_selection(format!("{}v{}", entity.index(), entity.generation()));
                }
                for part_id in &part_ids_in_box {
                    sm.add_to_selection(part_id.clone());
                }
            } else {
                // Replace selection with box contents.
                sm.clear();
                for part_id in &part_ids_in_box {
                    sm.add_to_selection(part_id.clone());
                }
            }
        }
    } else if mouse.just_released(MouseButton::Left) {
        // A pending marquee that never became ACTIVE is just a plain click —
        // do NOT clear here. `part_selection` already owns the click (it selects
        // the clicked part, or deselects on empty space); clearing on release
        // would wipe a single-click selection it just made. Only an actual drag
        // (the active branch above) changes the selection.
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

    // Draw EXACTLY the rect the selection scan used (viewport-local logical
    // pixels, computed from `ViewportBounds` in `handle_box_selection`). This
    // is WYSIWYG: the visible box == the selected region. (Recomputing here from
    // `get_viewport_x` used a different viewport-offset source, so the box was
    // shifted from what it selected — the "highlighting is very miss" report.)
    ui.set_box_select_visible(true);
    ui.set_box_select_x(box_state.rect_min.x);
    ui.set_box_select_y(box_state.rect_min.y);
    ui.set_box_select_w(box_state.rect_size.x);
    ui.set_box_select_h(box_state.rect_size.y);
}
