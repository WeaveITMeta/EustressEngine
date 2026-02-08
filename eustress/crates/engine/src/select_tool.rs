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
use crate::selection_box::SelectionBox;
use crate::rendering::PartEntity;
use crate::classes::{BasePart, Instance};
use crate::ui::{StudioState, Tool, BevySelectionManager};
use crate::math_utils::{
    calculate_rotated_aabb, ray_plane_intersection, ray_obb_intersection,
    align_to_surface, ray_intersects_part,
};

/// Drag threshold in pixels - must move this far to start dragging
const DRAG_THRESHOLD: f32 = 5.0;

/// Box selection threshold - must drag this far to start box select (in pixels)
const BOX_SELECT_THRESHOLD: f32 = 3.0;

/// Maximum raycast distance for surface detection
const MAX_RAYCAST_DISTANCE: f32 = 1000.0;

/// Smoothing factor for drag position (0 = no smoothing, 1 = max smoothing)
const DRAG_SMOOTHING: f32 = 0.3;

/// Minimum position change to apply (prevents micro-jitter)
const MIN_POSITION_CHANGE: f32 = 0.001;

/// Resource tracking the select tool drag state
#[derive(Resource)]
pub struct SelectToolState {
    pub dragging: bool,
    pub drag_started: bool, // Track if drag threshold was exceeded
    pub dragged_entity: Option<Entity>,
    pub drag_offset: Vec3,
    pub initial_position: Vec3,
    pub initial_cursor_pos: Vec2, // Track initial cursor position for threshold
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
                handle_select_drag,
                handle_box_selection,
                render_box_selection,
                debug_drag_gizmos,
            ).chain());
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
    state: Option<ResMut<SelectToolState>>,
    studio_state: Option<Res<StudioState>>,
    input: (Res<ButtonInput<MouseButton>>, Res<ButtonInput<KeyCode>>),
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    // Support both PartEntity (legacy) and Instance (modern) components
    mut selected_query: Query<(Entity, &mut Transform, &GlobalTransform, Option<&PartEntity>, Option<&Instance>, Option<&mut BasePart>), With<SelectionBox>>,
    all_parts_query: Query<(Entity, &GlobalTransform, &Mesh3d, Option<&PartEntity>, Option<&Instance>, Option<&BasePart>), Without<SelectionBox>>,
    // Query for children of selected entities (for Model support)
    hierarchy_queries: (Query<&Children>, Query<&ChildOf>),
    spatial_query: SpatialQuery,
    settings_and_undo: (Res<crate::editor_settings::EditorSettings>, ResMut<crate::undo::UndoStack>),
    // Tool states to check if clicking on handles
    tool_states: (Res<crate::move_tool::MoveToolState>, Res<crate::scale_tool::ScaleToolState>, Res<crate::rotate_tool::RotateToolState>),
) {
    let Some(mut state) = state else { return };
    let (mouse, keys) = input;
    let (children_query, parent_query) = hierarchy_queries;
    let (editor_settings, mut undo_stack) = settings_and_undo;
    let (move_state, scale_state, rotate_state) = tool_states;
    // Active with Select, Move, Scale, or Rotate tools
    let Some(studio_state) = studio_state else { return };
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
    
    // TODO: Check Slint UI focus state to block input when UI has focus
    
    let Ok(window) = windows.single() else { return; };
    let Some(cursor_pos) = window.cursor_position() else { return; };
    let Ok((camera, camera_transform)) = cameras.single() else { return; };
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
                let selection_size = bounds_max - bounds_min;
                let avg_size = selection_size.max_element();
                
                // MUST match move_tool.rs handle_length calculation exactly!
                let base_handle_length = if avg_size < 1.0 {
                    (avg_size * 1.2).max(0.3)
                } else {
                    (avg_size * 0.6).max(1.0)
                };
                let handle_length = base_handle_length + 0.5;
                
                // Check if clicking on move handle - let move_tool handle it
                if crate::move_tool::is_clicking_move_handle(&ray, center, Vec3::splat(avg_size), handle_length, camera_transform) {
                    return;
                }
                
                // Check if clicking on a selected part body - let move_tool handle free drag
                for (_entity, _, global_transform, _, _, basepart_opt) in selected_query.iter() {
                    let t = global_transform.compute_transform();
                    let size = basepart_opt.map(|bp| bp.size).unwrap_or(t.scale);
                    if ray_intersects_part_rotated(&ray, t.translation, t.rotation, size) {
                        // Clicking on selected part - move_tool handles free drag
                        return;
                    }
                }
            }
            // Not clicking on handle or selected part - continue to allow selecting new objects
        }
        
        // Check Scale tool handles (per-entity)
        if scale_state.active && studio_state.current_tool == Tool::Scale {
            for (_entity, _, global_transform, _, _, basepart_opt) in selected_query.iter() {
                let t = global_transform.compute_transform();
                let size = basepart_opt.map(|bp| bp.size).unwrap_or(t.scale);
                // MUST match scale_tool.rs handle_length calculation exactly!
                let scale_handle_length = (size.max_element() * 0.4) + 0.4;
                if crate::scale_tool::is_clicking_scale_handle(&ray, t.translation, t.rotation, size, scale_handle_length) {
                    // Clicking on scale handle - don't start drag, let scale tool handle it
                    return;
                }
            }
        }
        
        // Check Rotate tool handles (per-entity)
        if rotate_state.active && studio_state.current_tool == Tool::Rotate {
            for (_entity, _, global_transform, _, _, basepart_opt) in selected_query.iter() {
                let t = global_transform.compute_transform();
                let size = basepart_opt.map(|bp| bp.size).unwrap_or(t.scale);
                // MUST match rotate_tool.rs radius calculation exactly!
                let rotate_radius = (size.max_element() * 0.6).max(2.0).min(50.0);
                if crate::rotate_tool::is_clicking_rotate_handle(&ray, t.translation, rotate_radius, camera_transform) {
                    // Clicking on rotate handle - don't start drag, let rotate tool handle it
                    return;
                }
            }
        }
        
        // No tool handle clicked - check if clicking on a selected part to start dragging
        for (entity, transform, global_transform, _part_entity, _instance, basepart_opt) in &selected_query {
            let t = global_transform.compute_transform();
            let size = basepart_opt.map(|bp| bp.size).unwrap_or(t.scale);
            // Use precise OBB intersection with rotation for accurate click detection
            if ray_intersects_part_rotated(&ray, t.translation, t.rotation, size) {
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
                let excluded_entities: Vec<Entity> = selected_query.iter()
                    .map(|(e, _, _, _, _, _)| e)
                    .collect();

                // Retrieve leader initial state
                let initial_leader_pos = state.initial_positions.get(&dragged_entity).cloned().unwrap_or(Vec3::ZERO);
                let initial_leader_rot = state.initial_rotations.get(&dragged_entity).cloned().unwrap_or(Quat::IDENTITY);

                // We need the leader's size for offset calculation
                let leader_size = if let Ok((_, _, _, _, _, basepart_opt)) = selected_query.get(dragged_entity) {
                    basepart_opt.map(|bp| bp.size).unwrap_or(Vec3::ONE)
                } else {
                    Vec3::ONE
                };
                
                // 1. Find Surface
                let surface_hit = find_surface_with_physics(&spatial_query, &ray, &excluded_entities)
                    .map(|(pt, norm, ent)| (pt, norm, Some(ent)))
                    .or_else(|| find_surface_under_cursor_with_normal(&ray, &all_parts_query, &excluded_entities).map(|(pt, norm)| (pt, norm, None)));

                // 2. Calculate Target Position (NO rotation change - keep original orientation)
                // This provides predictable drag behavior without auto-alignment
                let target_pos = if let Some((hit_point, hit_normal, hit_entity)) = surface_hit {
                    // HIT A SURFACE - position on top of it but keep original rotation
                    state.last_hit_entity = hit_entity;
                    state.last_surface_normal = hit_normal;
                    state.debug_hit_point = Some(hit_point);
                    state.debug_hit_normal = Some(hit_normal);

                    // Calculate offset using the ORIGINAL rotation (not aligned)
                    let offset = calculate_surface_offset(&leader_size, &initial_leader_rot, &hit_normal);
                    
                    // Target position for the Leader's center - sit on surface
                    hit_point + hit_normal * (offset + 0.01)
                } else {
                    // NO SURFACE HIT - Drag on Ground Plane (Y=0)
                    state.debug_hit_point = None;
                    state.debug_hit_normal = None;

                    if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, Vec3::ZERO, Vec3::Y) {
                         let ground_pos = ray.origin + *ray.direction * t;
                         // Calculate offset using original rotation
                         let offset = calculate_surface_offset(&leader_size, &initial_leader_rot, &Vec3::Y);
                         ground_pos + Vec3::new(0.0, offset + 0.01, 0.0)
                    } else {
                        // Fallback: Use intersection with horizontal plane at leader's initial height
                         if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, initial_leader_pos, Vec3::Y) {
                             ray.origin + *ray.direction * t
                         } else {
                             initial_leader_pos
                         }
                    }
                };
                
                // No rotation change during drag
                let rotation_delta = Quat::IDENTITY;

                // Apply grid snapping if enabled (to position)
                let final_target_pos = if editor_settings.snap_enabled {
                    snap_to_grid(target_pos, editor_settings.snap_size)
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
                        let parent_entity = child_of.0;
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
                        let new_pos = final_target_pos + rotated_relative_pos;
                        
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
    
    // R and T key rotation while dragging (works with or without Ctrl)
    if state.dragging && state.dragged_entity.is_some() {
        // Check for R key
        let rotate_pressed = keys.just_pressed(KeyCode::KeyR);
        
        if rotate_pressed {
            // Calculate group center for rotation pivot (including children)
            let mut group_center = Vec3::ZERO;
            let mut count = 0;
            
            // Collect all entities to rotate (selected + their children)
            // But we only want to collect POSITIONS for center calc
            // The actual rotation should only be applied to TOP-LEVEL selected entities
            
            for (entity, transform, _, _, _, _) in selected_query.iter() {
                group_center += transform.translation;
                count += 1;
                
                // Also include children in center calculation
                if let Ok(children) = children_query.get(entity) {
                    for child in children.iter() {
                        if let Ok((_, child_global, _, _, _, _)) = all_parts_query.get(child) {
                            group_center += child_global.translation();
                            count += 1;
                        }
                    }
                }
            }
            
            if count > 0 {
                group_center /= count as f32;
            }
            
            let rotation = Quat::from_rotation_y(90.0_f32.to_radians());
            
            // Collect selected entities set for hierarchy check
            let selected_entities: std::collections::HashSet<Entity> = selected_query.iter().map(|(e, ..)| e).collect();
            
            // Rotate selected entities around group center
            for (entity, mut transform, _, _, _, _) in selected_query.iter_mut() {
                // Check if any ancestor is also selected
                let mut is_descendant = false;
                let mut current = entity;
                while let Ok(child_of) = parent_query.get(current) {
                    let parent_entity = child_of.0;
                    if selected_entities.contains(&parent_entity) {
                        is_descendant = true;
                        break;
                    }
                    current = parent_entity;
                }
                if is_descendant { continue; }

                // Rotate position around group center
                let relative_pos = transform.translation - group_center;
                let rotated_pos = rotation * relative_pos;
                transform.translation = group_center + rotated_pos;
                // Also rotate the entity itself
                transform.rotate_y(90.0_f32.to_radians());
            }
            
            // NO NEED TO MANUALLY ROTATE CHILDREN - they move with parents!
            // I removed the child iteration loop here.
            
            // Rotated 90° (Y axis)
        }
        
        // Check for T key
        let tilt_pressed = keys.just_pressed(KeyCode::KeyT);
        
        if tilt_pressed {
            // Tilt 90° on Z axis
            
            // Collect selected entities set for hierarchy check
            let selected_entities: std::collections::HashSet<Entity> = selected_query.iter().map(|(e, ..)| e).collect();

            for (entity, mut transform, _, _, _, _) in selected_query.iter_mut() {
                // Check if any ancestor is also selected
                let mut is_descendant = false;
                let mut current = entity;
                while let Ok(child_of) = parent_query.get(current) {
                    let parent_entity = child_of.0;
                    if selected_entities.contains(&parent_entity) {
                        is_descendant = true;
                        break;
                    }
                    current = parent_entity;
                }
                if is_descendant { continue; }

                transform.rotate_z(90.0_f32.to_radians());
            }
            
            // NO NEED TO MANUALLY ROTATE CHILDREN
            
            // Tilted 90° (Z axis)
        }
    }
    
    // +/- keys to move selected parts up/down by snap grid unit
    // Uses pressed() for key repeat when held down
    let has_selection = selected_query.iter().count() > 0;
    if has_selection {
        let snap_size = editor_settings.snap_size;
        
        // Use pressed() for continuous movement while key is held
        let move_up = keys.pressed(KeyCode::Minus) || 
                      keys.pressed(KeyCode::NumpadSubtract);
        
        let move_down = keys.pressed(KeyCode::Equal) || 
                        keys.pressed(KeyCode::NumpadAdd);
        
        if move_up {
            // Collect selected entities set for hierarchy check
            let selected_entities: std::collections::HashSet<Entity> = selected_query.iter().map(|(e, ..)| e).collect();

            for (entity, mut transform, _, _, _, _) in selected_query.iter_mut() {
                // Check if any ancestor is also selected
                let mut is_descendant = false;
                let mut current = entity;
                while let Ok(child_of) = parent_query.get(current) {
                    let parent_entity = child_of.0;
                    if selected_entities.contains(&parent_entity) {
                        is_descendant = true;
                        break;
                    }
                    current = parent_entity;
                }
                if is_descendant { continue; }

                transform.translation.y += snap_size;
            }
            // Moved up by snap_size
        }
        
        if move_down {
            // + key: SNAP TO SURFACE
            
            // Collect selected entities set for hierarchy check
            let selected_entities_set: std::collections::HashSet<Entity> = selected_query.iter().map(|(e, ..)| e).collect();
            
            // Collect entities to process (top-level only)
            let entities_data: Vec<(Entity, Vec3, Vec3)> = selected_query.iter()
                .filter(|(e, ..)| {
                    let mut is_descendant = false;
                    let mut current = *e;
                    while let Ok(child_of) = parent_query.get(current) {
                        let parent_entity = child_of.0;
                        if selected_entities_set.contains(&parent_entity) {
                            is_descendant = true;
                            break;
                        }
                        current = parent_entity;
                    }
                    !is_descendant
                })
                .map(|(e, t, _, _, _, bp)| {
                    let size = bp.map(|b| b.size).unwrap_or(Vec3::ONE);
                    (e, t.translation, size)
                })
                .collect();
            
            // Get list of selected entities to exclude from raycast (ALL selected)
            let excluded_entities: Vec<Entity> = selected_query.iter().map(|(e, ..)| e).collect();
            
            let mut _snapped_count = 0;
            for (entity, current_pos, size) in entities_data {
                let half_height = size.y * 0.5;
                
                // Cast a ray downward from the CENTER of the part
                let ray_origin = current_pos;
                let direction = Dir3::NEG_Y;
                
                if let Some(hit) = spatial_query.ray_hits(
                    ray_origin,
                    direction,
                    1000.0,
                    10,
                    true,
                    &SpatialQueryFilter::default().with_excluded_entities(excluded_entities.clone()),
                ).first() {
                    let surface_y = ray_origin.y - hit.distance;
                    let new_y = surface_y + half_height;
                    
                    if let Ok((_, mut transform, _, _, _, basepart_opt)) = selected_query.get_mut(entity) {
                        transform.translation.y = new_y;
                        if let Some(mut bp) = basepart_opt {
                            bp.cframe.translation.y = new_y;
                        }
                        _snapped_count += 1;
                    }
                } else {
                    if let Ok((_, mut transform, _, _, _, basepart_opt)) = selected_query.get_mut(entity) {
                        let new_y = half_height;
                        transform.translation.y = new_y;
                        if let Some(mut bp) = basepart_opt {
                            bp.cframe.translation.y = new_y;
                        }
                        _snapped_count += 1;
                    }
                }
            }
            
            // Snapped parts to surface
        }
    }
}

/// Calculate the vertical extent (half-height) of a part considering its rotation
/// This accounts for the fact that a rotated box's vertical footprint changes
fn calculate_vertical_extent(size: &Vec3, rotation: &Quat) -> f32 {
    // Transform each corner of the box by the rotation to find actual vertical extent
    // We only care about the vertical (Y) component
    let half_size = *size * 0.5;
    
    // The 8 corners of the box (in local space)
    let corners = [
        Vec3::new(half_size.x, half_size.y, half_size.z),
        Vec3::new(half_size.x, half_size.y, -half_size.z),
        Vec3::new(half_size.x, -half_size.y, half_size.z),
        Vec3::new(half_size.x, -half_size.y, -half_size.z),
        Vec3::new(-half_size.x, half_size.y, half_size.z),
        Vec3::new(-half_size.x, half_size.y, -half_size.z),
        Vec3::new(-half_size.x, -half_size.y, half_size.z),
        Vec3::new(-half_size.x, -half_size.y, -half_size.z),
    ];
    
    // Transform corners by rotation and find the max Y value
    let max_y = corners.iter()
        .map(|corner| rotation.mul_vec3(*corner).y)
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or(half_size.y);
    
    max_y.abs() // Return the positive vertical extent
}

/// Calculate the offset distance along a surface normal for proper surface snapping
/// This handles top, bottom, and side surfaces correctly, accounting for part rotation
fn calculate_surface_offset(size: &Vec3, rotation: &Quat, normal: &Vec3) -> f32 {
    let half_size = *size * 0.5;
    
    // Get the part's local axes in world space
    let local_x = rotation.mul_vec3(Vec3::X);
    let local_y = rotation.mul_vec3(Vec3::Y);
    let local_z = rotation.mul_vec3(Vec3::Z);
    
    // Calculate how far the part extends along the surface normal direction
    // by projecting each local axis onto the normal and multiplying by the half-extent
    let extent = (local_x.dot(*normal)).abs() * half_size.x
               + (local_y.dot(*normal)).abs() * half_size.y
               + (local_z.dot(*normal)).abs() * half_size.z;
    
    extent
}

/// Calculate the offset distance for a GROUP bounding box along a surface normal
/// The group bounding box is axis-aligned, so we just need the half-extent along the normal
fn calculate_group_surface_offset(group_size: &Vec3, normal: &Vec3) -> f32 {
    let half_size = *group_size * 0.5;
    
    // For an axis-aligned bounding box, the extent along a direction is simply
    // the sum of absolute dot products with each axis
    let extent = normal.x.abs() * half_size.x
               + normal.y.abs() * half_size.y
               + normal.z.abs() * half_size.z;
    
    extent
}

/// Check if ray intersects with part using precise OBB intersection (with rotation)
pub fn ray_intersects_part_rotated(ray: &Ray3d, position: Vec3, rotation: Quat, size: Vec3) -> bool {
    ray_obb_intersection(ray.origin, *ray.direction, position, size, rotation).is_some()
}

/// Ray-OBB intersection returning distance (for paste raycasting)
/// Works on ALL parts regardless of can_collide setting
pub fn ray_intersects_part_rotated_distance(ray: &Ray3d, position: Vec3, rotation: Quat, size: Vec3) -> Option<f32> {
    ray_obb_intersection(ray.origin, *ray.direction, position, size, rotation)
}

/// Find the surface under the cursor using Avian3D physics raycasting
/// 
/// This provides accurate collision detection against all physics colliders,
/// returning the exact hit point on the surface and the surface normal.
/// Excludes selected entities so dragging works properly.
/// Returns (hit_point, surface_normal, hit_entity)
fn find_surface_with_physics(
    spatial_query: &SpatialQuery,
    ray: &Ray3d,
    excluded_entities: &[Entity],
) -> Option<(Vec3, Vec3, Entity)> {
    let direction = Dir3::new(*ray.direction).unwrap_or(Dir3::NEG_Y);
    
    // Use filter to exclude dragged entities directly (more efficient)
    let filter = SpatialQueryFilter::default().with_excluded_entities(excluded_entities.to_vec());
    
    // Use ray_hits to get all intersections sorted by distance
    let hits = spatial_query.ray_hits(
        ray.origin,
        direction,
        MAX_RAYCAST_DISTANCE,
        20, // Max hits to check
        true, // solid
        &filter,
    );
    
    // Return the closest hit (already filtered and sorted)
    if let Some(hit) = hits.first() {
        let hit_point = ray.origin + *ray.direction * hit.distance;
        // The normal points OUTWARD from the surface we hit
        let hit_normal = hit.normal;
        
        // Ensure normal is valid (non-zero)
        if hit_normal.length_squared() > 0.001 {
            return Some((hit_point, hit_normal.normalize(), hit.entity));
        }
    }
    
    None
}

/// Find the surface under the cursor by raycasting against all other parts
/// Falls back to simple sphere intersection if no physics colliders are present
/// Uses entity list for reliable exclusion of dragged parts
#[allow(dead_code)]
fn find_surface_under_cursor(
    ray: &Ray3d,
    all_parts: &Query<(Entity, &GlobalTransform, &Mesh3d, Option<&PartEntity>, Option<&Instance>, Option<&BasePart>), Without<SelectionBox>>,
    excluded_entities: &[Entity],
) -> Option<Vec3> {
    let mut closest_hit: Option<(Vec3, f32)> = None;
    
    for (entity, transform, _mesh_handle, _part_entity, _instance, basepart) in all_parts.iter() {
        // Skip excluded entities (the parts being dragged)
        if excluded_entities.contains(&entity) {
            continue;
        }
        
        // Note: We DO NOT skip locked parts - we want to be able to drag onto locked surfaces
        // like the baseplate. Locked only prevents the part itself from being moved.
        
        // Get part transform and size
        let part_transform = transform.compute_transform();
        let part_pos = part_transform.translation;
        let part_size = basepart.map(|bp| bp.size).unwrap_or(part_transform.scale);
        
        // Use OBB intersection for precise surface detection
        if let Some(distance) = ray_obb_intersection(ray.origin, *ray.direction, part_pos, part_size, part_transform.rotation) {
            let _hit_point = ray.origin + *ray.direction * distance;
            
            // Calculate the top surface considering rotation
            // Use the same vertical extent calculation as dragging
            let vertical_extent = calculate_vertical_extent(&part_size, &part_transform.rotation);
            let top_surface = part_pos + Vec3::new(0.0, vertical_extent, 0.0);
            
            // Keep track of closest hit
            if closest_hit.is_none() || distance < closest_hit.unwrap().1 {
                closest_hit = Some((top_surface, distance));
            }
        }
    }
    
    closest_hit.map(|(pos, _)| pos)
}

/// Find the surface under the cursor with surface normal calculation
/// Returns (hit_point, surface_normal) for proper positioning
/// Uses entity list for reliable exclusion of dragged parts
fn find_surface_under_cursor_with_normal(
    ray: &Ray3d,
    all_parts: &Query<(Entity, &GlobalTransform, &Mesh3d, Option<&PartEntity>, Option<&Instance>, Option<&BasePart>), Without<SelectionBox>>,
    excluded_entities: &[Entity],
) -> Option<(Vec3, Vec3)> {
    let mut closest_hit: Option<(Vec3, Vec3, f32)> = None; // (hit_point, normal, distance)
    
    for (entity, transform, _mesh_handle, _part_entity, _instance, basepart) in all_parts.iter() {
        // Skip excluded entities (the parts being dragged)
        if excluded_entities.contains(&entity) {
            continue;
        }
        
        // Get part transform and size
        let part_transform = transform.compute_transform();
        let part_pos = part_transform.translation;
        let part_rot = part_transform.rotation;
        let part_size = basepart.map(|bp| bp.size).unwrap_or(part_transform.scale);
        
        // Use OBB intersection for precise surface detection
        if let Some(distance) = ray_obb_intersection(ray.origin, *ray.direction, part_pos, part_size, part_rot) {
            let hit_point = ray.origin + *ray.direction * distance;
            
            // Calculate which face was hit based on the hit point relative to the box center
            // Transform hit point to local space
            let local_hit = part_rot.inverse() * (hit_point - part_pos);
            let half_size = part_size * 0.5;
            
            // Determine which face is closest (the one the ray hit)
            let mut best_face_normal = Vec3::Y; // Default to top
            let mut best_face_dist = f32::MAX;
            
            // Check each face
            let faces = [
                (Vec3::X, half_size.x - local_hit.x),    // +X face
                (Vec3::NEG_X, half_size.x + local_hit.x), // -X face
                (Vec3::Y, half_size.y - local_hit.y),    // +Y face (top)
                (Vec3::NEG_Y, half_size.y + local_hit.y), // -Y face (bottom)
                (Vec3::Z, half_size.z - local_hit.z),    // +Z face
                (Vec3::NEG_Z, half_size.z + local_hit.z), // -Z face
            ];
            
            for (normal, dist) in faces {
                if dist.abs() < best_face_dist {
                    best_face_dist = dist.abs();
                    best_face_normal = normal;
                }
            }
            
            // Transform normal back to world space
            let world_normal = (part_rot * best_face_normal).normalize();
            
            // Keep track of closest hit
            if closest_hit.is_none() || distance < closest_hit.unwrap().2 {
                closest_hit = Some((hit_point, world_normal, distance));
            }
        }
    }
    
    closest_hit.map(|(pos, normal, _)| (pos, normal))
}

/// Snap a position to the nearest grid point
fn snap_to_grid(position: Vec3, grid_size: f32) -> Vec3 {
    Vec3::new(
        (position.x / grid_size).round() * grid_size,
        (position.y / grid_size).round() * grid_size,
        (position.z / grid_size).round() * grid_size,
    )
}

// ============================================================================
// Box Selection Systems
// ============================================================================

/// System to handle box selection (drag to select multiple entities)
fn handle_box_selection(
    mut box_state: ResMut<BoxSelectionState>,
    select_state: Option<Res<SelectToolState>>,
    studio_state: Option<Res<StudioState>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    selection_manager: Option<Res<BevySelectionManager>>,
    // Query entities with PartEntity OR Instance (supports both legacy and modern)
    parts_query: Query<(Entity, &GlobalTransform, Option<&BasePart>, Option<&PartEntity>, Option<&Instance>), Or<(With<PartEntity>, With<Instance>)>>,
    selected_query: Query<Entity, With<SelectionBox>>,
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
    
    // TODO: Check Slint UI focus state to block input when UI has focus
    
    let Ok(window) = windows.single() else { return; };
    let Some(cursor_pos) = window.cursor_position() else { return; };
    let Ok((camera, camera_transform)) = cameras.single() else { return; };
    
    // Check if Shift is held for additive selection
    let shift_held = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    
    if mouse.just_pressed(MouseButton::Left) {
        // Reset any stale state from previous interactions
        box_state.active = false;
        box_state.pending = false;
        
        // Don't start box select if we're already dragging a part
        if select_state.dragging {
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
        
        if !clicking_on_part {
            // Start potential box selection - mark as pending
            box_state.pending = true;
            box_state.start_pos = cursor_pos;
            box_state.current_pos = cursor_pos;
            box_state.additive = shift_held;
            debug!("📦 Box selection pending at {:?}", cursor_pos);
            
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
            if !box_state.active {
                info!("📦 Box selection ACTIVE - dragging from {:?} to {:?}", box_state.start_pos, cursor_pos);
            }
            box_state.active = true;
            box_state.current_pos = cursor_pos;
            
            // Calculate screen-space bounding box
            let min_x = box_state.start_pos.x.min(cursor_pos.x);
            let max_x = box_state.start_pos.x.max(cursor_pos.x);
            let min_y = box_state.start_pos.y.min(cursor_pos.y);
            let max_y = box_state.start_pos.y.max(cursor_pos.y);
            
            // Find all part_ids within the box
            let mut part_ids_in_box: Vec<String> = Vec::new();
            
            for (entity, transform, basepart, part_entity, instance) in parts_query.iter() {
                // Skip locked parts - they shouldn't be selectable via box selection
                if let Some(bp) = basepart {
                    if bp.locked {
                        continue;
                    }
                }
                
                // Project entity position to screen space
                let world_pos = transform.translation();
                if let Ok(screen_pos) = camera.world_to_viewport(camera_transform, world_pos) {
                    // Check if entity center is within the box
                    if screen_pos.x >= min_x && screen_pos.x <= max_x &&
                       screen_pos.y >= min_y && screen_pos.y <= max_y {
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

/// System to render the box selection rectangle
/// TODO: Implement with Bevy gizmos or Slint overlay
fn render_box_selection(
    _box_state: Res<BoxSelectionState>,
) {
    // Box selection rendering will be handled by Slint UI overlay
    // The selection logic still works, just the visual rectangle is not drawn
}
