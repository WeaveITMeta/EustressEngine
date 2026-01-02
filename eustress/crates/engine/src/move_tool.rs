#![allow(dead_code)]
#![allow(unused_variables)]

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::EguiContexts;
use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use crate::selection_box::SelectionBox;
use crate::editor_settings::EditorSettings;
use crate::gizmo_tools::TransformGizmoGroup;
use crate::math_utils::{ray_plane_intersection, ray_to_line_segment_distance, ray_intersects_part};

/// Resource tracking the move tool state
#[derive(Resource)]
pub struct MoveToolState {
    pub active: bool,
    pub dragged_axis: Option<Axis3d>,
    pub initial_positions: std::collections::HashMap<Entity, Vec3>, // Store all selected parts' positions
    pub initial_rotations: std::collections::HashMap<Entity, Quat>, // Store all selected parts' rotations
    pub group_center: Vec3, // Center of all selected parts for gizmo positioning
    pub initial_mouse_world: Vec3,
    pub drag_start_pos: Vec2,
    pub free_drag: bool, // True when dragging part body (not axis handle)
    pub dragged_entity: Option<Entity>, // The specific entity being dragged (for alignment leader)
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

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Axis3d {
    X,
    Y,
    Z,
}

impl Axis3d {
    fn to_vec3(&self) -> Vec3 {
        match self {
            Axis3d::X => Vec3::X,
            Axis3d::Y => Vec3::Y,
            Axis3d::Z => Vec3::Z,
        }
    }
    
    fn color(&self) -> Color {
        match self {
            Axis3d::X => Color::srgb(1.0, 0.0, 0.0), // Red
            Axis3d::Y => Color::srgb(0.0, 1.0, 0.0), // Green
            Axis3d::Z => Color::srgb(0.0, 0.0, 1.0), // Blue
        }
    }
}

/// Plugin for the move tool functionality
pub struct MoveToolPlugin;

impl Plugin for MoveToolPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<MoveToolState>()
            .add_systems(Update, (
                manage_tool_activation,
                draw_move_gizmos,
                handle_move_interaction,
            ));
    }
}

/// System to draw axis gizmos for selected entities
fn draw_move_gizmos(
    mut gizmos: Gizmos<TransformGizmoGroup>,
    state: Res<MoveToolState>,
    query: Query<(Entity, &GlobalTransform, Option<&crate::classes::BasePart>), With<SelectionBox>>,
    // Query for children of selected entities (for Model support)
    children_query: Query<&Children>,
    child_transforms: Query<(&GlobalTransform, Option<&crate::classes::BasePart>)>,
) {
    if !state.active {
        return;
    }
    
    // Don't draw if no parts are selected
    if query.is_empty() {
        return;
    }
    
    // Calculate combined bounding box of all selected parts + their children
    let mut bounds_min = Vec3::splat(f32::MAX);
    let mut bounds_max = Vec3::splat(f32::MIN);
    let mut count = 0;
    
    for (entity, global_transform, base_part) in &query {
        let transform = global_transform.compute_transform();
        
        // Include the selected part itself
        let size = base_part.map(|bp| bp.size).unwrap_or(transform.scale);
        let half_size = size * 0.5;
        let (part_min, part_max) = calculate_rotated_aabb(transform.translation, transform.rotation, half_size);
        bounds_min = bounds_min.min(part_min);
        bounds_max = bounds_max.max(part_max);
        count += 1;
        
        // Include children (recursive check would be better, but 1 level is a start)
        if let Ok(children) = children_query.get(entity) {
            for child in children.iter() {
                if let Ok((child_global, child_bp)) = child_transforms.get(child) {
                    let child_t = child_global.compute_transform();
                    let child_size = child_bp.map(|bp| bp.size).unwrap_or(child_t.scale);
                    let child_half = child_size * 0.5;
                    let (c_min, c_max) = calculate_rotated_aabb(child_t.translation, child_t.rotation, child_half);
                    bounds_min = bounds_min.min(c_min);
                    bounds_max = bounds_max.max(c_max);
                    count += 1;
                }
            }
        }
    }
    
    // Additional safety check - don't draw if no valid transforms
    if count == 0 {
        return;
    }
    
    // Center is the center of the combined AABB
    let center = (bounds_min + bounds_max) * 0.5;
    // Size for handles depends on the size of the selection
    let selection_size = bounds_max - bounds_min;
    let avg_size = selection_size.max_element();
    
    // Draw ONE gizmo at the group center
    let pos = center;
    // IMPROVED SCALING: Better proportional scaling for small objects
    // For small objects, be more aggressive with scaling; for large objects, be more conservative
    let base_handle_length = if avg_size < 1.0 {
        // Small objects: scale more aggressively, minimum 0.3 instead of 1.0
        (avg_size * 1.2).max(0.3)
    } else {
        // Large objects: scale conservatively, minimum 1.0
        (avg_size * 0.6).max(1.0)
    };
    let handle_length = base_handle_length + 0.5; // Reduced constant padding from 1.5 to 0.5
    
    // Scale arrows more aggressively for small objects
    let arrow_scale_factor = if avg_size < 1.0 {
        // Small objects: scale arrows down more
        (avg_size * 0.3).max(0.05)
    } else {
        // Large objects: normal scaling
        0.15 * (1.0 + avg_size * 0.05).min(2.0)
    };
    let arrow_size = arrow_scale_factor;
    let arrow_length = arrow_scale_factor * 1.5;
    
    // Helper to draw an arrow (cone) pointing in a direction
    let draw_arrow = |gizmos: &mut Gizmos<TransformGizmoGroup>, tip: Vec3, direction: Vec3, size: f32, length: f32, color: Color| {
        // Draw a cone/arrow shape using lines from tip to base circle
        let base_center = tip - direction * length;
        let segments = 8;
        
        // Get perpendicular vectors for the cone base
        let up = if direction.abs().dot(Vec3::Y) > 0.9 { Vec3::X } else { Vec3::Y };
        let right = direction.cross(up).normalize() * size;
        let forward = direction.cross(right).normalize() * size;
        
        // Draw lines from tip to base circle points
        for i in 0..segments {
            let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
            let base_point = base_center + right * angle.cos() + forward * angle.sin();
            gizmos.line(tip, base_point, color);
            
            // Connect base circle points
            let next_angle = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
            let next_base_point = base_center + right * next_angle.cos() + forward * next_angle.sin();
            gizmos.line(base_point, next_base_point, color);
        }
    };
    
    let highlight_x = state.dragged_axis == Some(Axis3d::X);
    let highlight_y = state.dragged_axis == Some(Axis3d::Y);
    let highlight_z = state.dragged_axis == Some(Axis3d::Z);
    
    let red = Color::srgb(1.0, 0.0, 0.0);
    let green = Color::srgb(0.0, 1.0, 0.0);
    let blue = Color::srgb(0.0, 0.0, 1.0);
    let yellow = Color::srgb(1.0, 1.0, 0.0);
    
    // X axis (Red) - BOTH directions with arrows
    let x_end_pos = pos + Vec3::X * handle_length;
    let x_end_neg = pos - Vec3::X * handle_length;
    let x_color = if highlight_x { yellow } else { red };
    let x_arrow_size = if highlight_x { arrow_size * 1.2 } else { arrow_size };
    gizmos.line(pos, x_end_pos, x_color);
    draw_arrow(&mut gizmos, x_end_pos, Vec3::X, x_arrow_size, arrow_length, x_color);
    gizmos.line(pos, x_end_neg, x_color);
    draw_arrow(&mut gizmos, x_end_neg, -Vec3::X, x_arrow_size, arrow_length, x_color);
    
    // Y axis (Green) - BOTH directions with arrows
    let y_end_pos = pos + Vec3::Y * handle_length;
    let y_end_neg = pos - Vec3::Y * handle_length;
    let y_color = if highlight_y { yellow } else { green };
    let y_arrow_size = if highlight_y { arrow_size * 1.2 } else { arrow_size };
    gizmos.line(pos, y_end_pos, y_color);
    draw_arrow(&mut gizmos, y_end_pos, Vec3::Y, y_arrow_size, arrow_length, y_color);
    gizmos.line(pos, y_end_neg, y_color);
    draw_arrow(&mut gizmos, y_end_neg, -Vec3::Y, y_arrow_size, arrow_length, y_color);
    
    // Z axis (Blue) - BOTH directions with arrows
    let z_end_pos = pos + Vec3::Z * handle_length;
    let z_end_neg = pos - Vec3::Z * handle_length;
    let z_color = if highlight_z { yellow } else { blue };
    let z_arrow_size = if highlight_z { arrow_size * 1.2 } else { arrow_size };
    gizmos.line(pos, z_end_pos, z_color);
    draw_arrow(&mut gizmos, z_end_pos, Vec3::Z, z_arrow_size, arrow_length, z_color);
    gizmos.line(pos, z_end_neg, z_color);
    draw_arrow(&mut gizmos, z_end_neg, -Vec3::Z, z_arrow_size, arrow_length, z_color);
}

/// Calculate the best plane normal for axis-constrained movement
/// Uses a plane that contains the axis but is as perpendicular to the camera as possible
fn get_axis_drag_plane_normal(axis: Vec3, camera_forward: Vec3) -> Vec3 {
    // Get a vector perpendicular to both the axis and camera forward
    let perp = axis.cross(camera_forward);
    
    // If perp is near zero, camera is looking along the axis - use camera up instead
    if perp.length_squared() < 0.001 {
        // Fallback: use a plane perpendicular to camera
        return camera_forward;
    }
    
    // The plane normal is perpendicular to both the axis and this perpendicular
    // This gives us a plane that contains the axis and faces the camera
    axis.cross(perp).normalize()
}

/// System to manage tool activation states based on current tool selection
fn manage_tool_activation(
    mut move_state: ResMut<MoveToolState>,
    mut scale_state: ResMut<crate::scale_tool::ScaleToolState>,
    mut rotate_state: ResMut<crate::rotate_tool::RotateToolState>,
    studio_state: Res<crate::ui::StudioState>,
) {
    use crate::ui::Tool;
    
    // Activate/deactivate tools based on current tool selection
    move_state.active = studio_state.current_tool == Tool::Move;
    scale_state.active = studio_state.current_tool == Tool::Scale;
    rotate_state.active = studio_state.current_tool == Tool::Rotate;
}

/// Handle mouse interaction for moving selected entities
fn handle_move_interaction(
    mut state: ResMut<MoveToolState>,
    settings: Res<EditorSettings>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut egui_ctx: EguiContexts,
    mut query: Query<(Entity, &GlobalTransform, &mut Transform, Option<&mut crate::classes::BasePart>), With<SelectionBox>>,
    // Query for children of selected entities (for Model support - bounds only)
    children_query: Query<&Children>,
    // UPDATED: Include Option<&BasePart> to get accurate size for children
    child_global_transforms: Query<(&GlobalTransform, Option<&crate::classes::BasePart>), Without<SelectionBox>>,
    // Query for unselected parts to raycast against (collision candidates)
    unselected_query: Query<(Entity, &GlobalTransform, Option<&crate::classes::BasePart>), Without<SelectionBox>>,
    // Query for parents to check hierarchy relationships (ChildOf in Bevy 0.17)
    parent_query: Query<&ChildOf>,
    // Undo stack for recording transform changes
    mut undo_stack: ResMut<crate::undo::UndoStack>,
    // Physics spatial query for surface snapping
    spatial_query: SpatialQuery,
) {
    if !state.active {
        return;
    }
    
    // Skip if cursor is over egui UI
    let Ok(ctx) = egui_ctx.ctx_mut() else { return; };
    if ctx.wants_pointer_input() {
        return;
    }
    
    let Ok(window) = windows.single() else { return; };
    let Some(cursor_pos) = window.cursor_position() else { return; };
    
    let Ok((camera, camera_transform)) = cameras.single() else { return; };
    
    // Get ray from cursor
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else { return; };
    
    // Get camera forward direction for plane calculation
    let camera_forward = camera_transform.forward().as_vec3();
    
    if mouse.just_pressed(MouseButton::Left) {
        // Calculate group center for hit detection (same as draw function)
        let mut bounds_min = Vec3::splat(f32::MAX);
        let mut bounds_max = Vec3::splat(f32::MIN);
        let mut count = 0;
        
        let query_iter: Vec<_> = query.iter().collect();
        if query_iter.is_empty() {
            return;
        }
        
        for (entity, global_transform, transform, base_part) in &query_iter {
            // Include the selected part itself (Use GLOBAL transform for bounds)
            let t = global_transform.compute_transform();
            let size = base_part.as_ref().map(|bp| bp.size).unwrap_or(t.scale);
            
            // Use precise OBB intersection check (handles rotation)
            let half_size = size * 0.5;
            let (part_min, part_max) = calculate_rotated_aabb(t.translation, t.rotation, half_size);
            bounds_min = bounds_min.min(part_min);
            bounds_max = bounds_max.max(part_max);
            count += 1;
            
            // Include children (Use GLOBAL transform for bounds)
            if let Ok(children) = children_query.get(*entity) {
                for child in children.iter() {
                    if let Ok((child_global, child_bp)) = child_global_transforms.get(child) {
                        let child_t = child_global.compute_transform();
                        // Use BasePart size if available, otherwise scale
                        let child_size = child_bp.map(|bp| bp.size).unwrap_or(child_t.scale);
                        
                        let child_half = child_size * 0.5;
                        let (c_min, c_max) = calculate_rotated_aabb(child_t.translation, child_t.rotation, child_half);
                        bounds_min = bounds_min.min(c_min);
                        bounds_max = bounds_max.max(c_max);
                        count += 1;
                    }
                }
            }
        }
        
        if count == 0 {
            return;
        }
        
        // Center is the center of the combined AABB
        let center = (bounds_min + bounds_max) * 0.5;
        // Size for handles depends on the size of the selection
        let selection_size = bounds_max - bounds_min;
        let avg_size = selection_size.max_element();
        
        // MATCH DRAW FUNCTION: Use same improved scaling logic
        let base_handle_length = if avg_size < 1.0 {
            // Small objects: scale more aggressively, minimum 0.3
            (avg_size * 1.2).max(0.3)
        } else {
            // Large objects: scale conservatively, minimum 1.0
            (avg_size * 0.6).max(1.0)
        };
        let handle_length = base_handle_length + 0.5;
        
        // Check if clicking on an axis handle at the group center
        if let Some(axis) = detect_axis_hit(&ray, center, Vec3::splat(avg_size), handle_length, camera_transform) {
            state.dragged_axis = Some(axis);
            state.free_drag = false;
            state.group_center = center;
            state.drag_start_pos = cursor_pos;
            
            // Store initial positions of ALL selected parts (Children move automatically via Bevy)
            state.initial_positions.clear();
            state.initial_rotations.clear();
            
            // Identify the dragged entity (leader)
            state.dragged_entity = None;
            
            for (entity, _, transform, _) in &query_iter {
                state.initial_positions.insert(*entity, transform.translation);
                state.initial_rotations.insert(*entity, transform.rotation);
            }
            
            // Calculate initial world position at drag start
            // Use a plane that contains the axis but faces the camera for reliable intersection
            let plane_normal = get_axis_drag_plane_normal(axis.to_vec3(), camera_forward);
            if let Some(world_pos) = ray_plane_intersection(&ray, center, plane_normal) {
                state.initial_mouse_world = world_pos;
            }
        } else {
            // No axis handle hit - check if clicking on a selected part for free drag
            for (entity, global_transform, transform, base_part) in &query_iter {
                let t = global_transform.compute_transform();
                let size = base_part.as_ref().map(|bp| bp.size).unwrap_or(t.scale);
                
                // Check if ray intersects this part
                if ray_intersects_part(&ray, t.translation, size) {
                    // Start free drag mode
                    state.free_drag = true;
                    state.dragged_axis = None;
                    state.group_center = center;
                    state.drag_start_pos = cursor_pos;
                    state.dragged_entity = Some(*entity);
                    
                    // Store initial positions
                    state.initial_positions.clear();
                    state.initial_rotations.clear();
                    for (e, _, tr, _) in &query_iter {
                        state.initial_positions.insert(*e, tr.translation);
                        state.initial_rotations.insert(*e, tr.rotation);
                    }
                    
                    // Calculate initial world position using horizontal plane at group center
                    let plane_normal = Vec3::Y;
                    if let Some(world_pos) = ray_plane_intersection(&ray, center, plane_normal) {
                        state.initial_mouse_world = world_pos;
                    }
                    break;
                }
            }
        }
    } else if mouse.pressed(MouseButton::Left) {
        if let Some(axis) = state.dragged_axis {
            // Axis-constrained drag mode
            let axis_vec = axis.to_vec3();
            
            // Use the same plane calculation as drag start for consistency
            let plane_normal = get_axis_drag_plane_normal(axis_vec, camera_forward);
            if let Some(current_world_pos) = ray_plane_intersection(&ray, state.group_center, plane_normal) {
                // Calculate delta in world space
                let delta = current_world_pos - state.initial_mouse_world;
                
                // Project delta onto the selected axis
                let axis_delta = delta.dot(axis_vec) * axis_vec;
                
                // Apply snapping using editor settings
                let snapped_delta = settings.apply_snap_vec3(axis_delta);
                
                // Collect selected entities for exclusion from raycasts
                let selected_entities: Vec<Entity> = query.iter().map(|(e, ..)| e).collect();
                let selected_set: std::collections::HashSet<Entity> = selected_entities.iter().copied().collect();

                // Move ALL selected parts by the same delta
                for (entity, global_transform, mut transform, base_part_opt) in query.iter_mut() {
                    // Check if any ancestor is also selected
                    let mut is_descendant = false;
                    let mut current = entity;
                    while let Ok(child_of) = parent_query.get(current) {
                        let parent_entity = child_of.0;
                        if selected_set.contains(&parent_entity) {
                            is_descendant = true;
                            break;
                        }
                        current = parent_entity;
                    }
                    if is_descendant { continue; }

                    if let Some(initial_pos) = state.initial_positions.get(&entity) {
                        let mut new_pos = *initial_pos + snapped_delta;
                        
                        // Surface snapping: raycast from cursor to find surfaces to snap onto
                        // This allows placing parts on top of other parts
                        if settings.surface_snap_enabled {
                            // Get the part's size for proper offset calculation
                            let part_size = base_part_opt.as_ref()
                                .map(|bp| bp.size)
                                .unwrap_or(global_transform.compute_transform().scale);
                            let half_height = part_size.y * 0.5;
                            
                            // Raycast from cursor position to find surfaces
                            let filter = SpatialQueryFilter::default()
                                .with_excluded_entities(selected_entities.clone());
                            
                            // Cast ray from cursor into scene
                            if let Ok(direction) = Dir3::new(*ray.direction) {
                                let hits = spatial_query.ray_hits(
                                    ray.origin,
                                    direction,
                                    1000.0,
                                    10,
                                    true,
                                    &filter,
                                );
                                
                                if let Some(hit) = hits.first() {
                                    let hit_point = ray.origin + *ray.direction * hit.distance;
                                    let hit_normal = hit.normal;
                                    
                                    // If we hit a surface, snap the part to sit on top of it
                                    // Use the surface normal to determine offset direction
                                    if hit_normal.length_squared() > 0.001 {
                                        let normal = hit_normal.normalize();
                                        
                                        // Calculate offset based on which face we're placing on
                                        // For top surfaces (normal pointing up), offset by half height
                                        let offset = if normal.y > 0.7 {
                                            // Top surface - place part on top
                                            half_height
                                        } else if normal.y < -0.7 {
                                            // Bottom surface - place part below
                                            -half_height
                                        } else {
                                            // Side surface - use half of the appropriate dimension
                                            if normal.x.abs() > normal.z.abs() {
                                                part_size.x * 0.5
                                            } else {
                                                part_size.z * 0.5
                                            }
                                        };
                                        
                                        // Snap to surface with proper offset
                                        new_pos = hit_point + normal * offset;
                                        
                                        // Apply grid snapping to the surface-snapped position
                                        new_pos = settings.apply_snap_vec3(new_pos);
                                    }
                                }
                            }
                        }
                        
                        transform.translation = new_pos;
                        
                        // Also update BasePart.cframe to keep it in sync
                        if let Some(mut bp) = base_part_opt {
                            bp.cframe.translation = new_pos;
                        }
                    }
                }
            }
        } else if state.free_drag {
            // Free drag mode - move on horizontal plane
            let plane_normal = Vec3::Y;
            if let Some(current_world_pos) = ray_plane_intersection(&ray, state.group_center, plane_normal) {
                let delta = current_world_pos - state.initial_mouse_world;
                
                // Apply snapping
                let snapped_delta = settings.apply_snap_vec3(delta);
                
                // Collect selected entities for exclusion
                let selected_entities: Vec<Entity> = query.iter().map(|(e, ..)| e).collect();
                let selected_set: std::collections::HashSet<Entity> = selected_entities.iter().copied().collect();
                
                // Move all selected parts
                for (entity, global_transform, mut transform, base_part_opt) in query.iter_mut() {
                    // Skip if ancestor is also selected
                    let mut is_descendant = false;
                    let mut current = entity;
                    while let Ok(child_of) = parent_query.get(current) {
                        let parent_entity = child_of.0;
                        if selected_set.contains(&parent_entity) {
                            is_descendant = true;
                            break;
                        }
                        current = parent_entity;
                    }
                    if is_descendant { continue; }
                    
                    if let Some(initial_pos) = state.initial_positions.get(&entity) {
                        let new_pos = *initial_pos + snapped_delta;
                        transform.translation = new_pos;
                        
                        if let Some(mut bp) = base_part_opt {
                            bp.cframe.translation = new_pos;
                        }
                    }
                }
            }
        }
    } else if mouse.just_released(MouseButton::Left) {
        // Record undo action if we were dragging (axis or free)
        if (state.dragged_axis.is_some() || state.free_drag) && !state.initial_positions.is_empty() {
            let mut old_transforms: Vec<(u64, [f32; 3], [f32; 4])> = Vec::new();
            let mut new_transforms: Vec<(u64, [f32; 3], [f32; 4])> = Vec::new();
            
            for (entity, _, transform, _) in query.iter() {
                if let Some(initial_pos) = state.initial_positions.get(&entity) {
                    if let Some(initial_rot) = state.initial_rotations.get(&entity) {
                        // Only record if position actually changed
                        let pos_changed = (*initial_pos - transform.translation).length() > 0.001;
                        
                        if pos_changed {
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
        
        state.dragged_axis = None;
        state.free_drag = false;
        state.initial_positions.clear();
        state.initial_rotations.clear();
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

// Re-export calculate_rotated_aabb for external use (part_selection, select_tool)
pub use crate::math_utils::calculate_rotated_aabb;

/// Detect if a ray hits one of the axis handles
/// Returns the axis that was hit, prioritizing the closest axis to the camera
fn detect_axis_hit(
    ray: &Ray3d,
    center: Vec3,
    _size: Vec3,
    handle_length: f32,
    camera_transform: &GlobalTransform,
) -> Option<Axis3d> {
    // DYNAMIC HIT RADIUS: Scale based on handle length for small objects
    // Minimum 0.15 for tiny objects, scales up to 0.4 for larger objects
    let hit_radius = (handle_length * 0.25).clamp(0.15, 0.5);
    
    // Check each axis and find the closest hit
    let mut best_hit: Option<(Axis3d, f32)> = None;
    
    for axis in [Axis3d::X, Axis3d::Y, Axis3d::Z] {
        let axis_vec = axis.to_vec3();
        
        // Check both positive and negative directions
        for direction in [1.0_f32, -1.0_f32] {
            let axis_start = center;
            let axis_end = center + axis_vec * handle_length * direction;
            
            // Find closest point on ray to the axis line segment
            if let Some(dist) = ray_to_line_segment_distance(ray, axis_start, axis_end) {
                if dist < hit_radius {
                    // Calculate distance from camera to the axis for depth sorting
                    let axis_center = (axis_start + axis_end) * 0.5;
                    let camera_dist = (axis_center - camera_transform.translation()).length();
                    
                    if best_hit.is_none() || camera_dist < best_hit.unwrap().1 {
                        best_hit = Some((axis, camera_dist));
                    }
                }
            }
        }
    }
    
    best_hit.map(|(axis, _)| axis)
}

/// Public function to check if a ray is clicking on a move handle
/// Used by other modules (part_selection, select_tool) to avoid interfering with move tool
pub fn is_clicking_move_handle(
    ray: &Ray3d,
    center: Vec3,
    size: Vec3,
    handle_length: f32,
    camera_transform: &GlobalTransform,
) -> bool {
    detect_axis_hit(ray, center, size, handle_length, camera_transform).is_some()
}
