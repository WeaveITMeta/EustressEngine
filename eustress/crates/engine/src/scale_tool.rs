#![allow(dead_code)]
#![allow(unused_variables)]

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
#[allow(unused_imports)]
use bevy::gizmos::config::{GizmoConfigStore, DefaultGizmoConfigGroup};
use crate::selection_box::SelectionBox;
use crate::gizmo_tools::TransformGizmoGroup;
use crate::math_utils::{ray_plane_intersection, ray_to_point_distance, ray_intersects_part};

/// Resource tracking the scale tool state
#[derive(Resource, Default)]
pub struct ScaleToolState {
    pub active: bool,
    pub dragged_axis: Option<ScaleAxis>,
    pub initial_scale: Vec3,  // For single entity (backward compat)
    pub initial_position: Vec3,  // For single entity (backward compat)
    pub drag_start_pos: Vec2,
    pub initial_mouse_world: Vec3,
    pub dragged_entity: Option<Entity>, // Entity being dragged
    pub initial_scales: std::collections::HashMap<Entity, Vec3>, // For multi-select
    pub initial_positions: std::collections::HashMap<Entity, Vec3>, // For multi-select
    pub group_center: Vec3, // Center of all selected parts for gizmo positioning
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum ScaleAxis {
    XPos,      // Positive X direction handle
    XNeg,      // Negative X direction handle
    YPos,      // Positive Y direction handle
    YNeg,      // Negative Y direction handle
    ZPos,      // Positive Z direction handle
    ZNeg,      // Negative Z direction handle
    Uniform,   // Center handle (uniform scaling)
}

impl ScaleAxis {
    fn axis_type(&self) -> &str {
        match self {
            ScaleAxis::XPos | ScaleAxis::XNeg => "X",
            ScaleAxis::YPos | ScaleAxis::YNeg => "Y",
            ScaleAxis::ZPos | ScaleAxis::ZNeg => "Z",
            ScaleAxis::Uniform => "Uniform",
        }
    }
    
    fn direction(&self) -> f32 {
        match self {
            ScaleAxis::XPos | ScaleAxis::YPos | ScaleAxis::ZPos => 1.0,
            ScaleAxis::XNeg | ScaleAxis::YNeg | ScaleAxis::ZNeg => -1.0,
            ScaleAxis::Uniform => 1.0,
        }
    }
    
    fn color(&self) -> Color {
        match self {
            ScaleAxis::XPos | ScaleAxis::XNeg => Color::srgb(1.0, 0.0, 0.0), // Red
            ScaleAxis::YPos | ScaleAxis::YNeg => Color::srgb(0.0, 1.0, 0.0), // Green
            ScaleAxis::ZPos | ScaleAxis::ZNeg => Color::srgb(0.0, 0.0, 1.0), // Blue
            ScaleAxis::Uniform => Color::srgb(1.0, 1.0, 1.0), // White
        }
    }
}

/// Plugin for the scale tool functionality
pub struct ScaleToolPlugin;

impl Plugin for ScaleToolPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<ScaleToolState>()
            .add_systems(Update, (
                draw_scale_gizmos,
                handle_scale_interaction,
            ));
    }
}

/// System to draw scale handle gizmos for selected entities
fn draw_scale_gizmos(
    mut gizmos: Gizmos<TransformGizmoGroup>,
    state: Res<ScaleToolState>,
    query: Query<(&GlobalTransform, Option<&crate::classes::BasePart>), With<SelectionBox>>,
) {
    if !state.active {
        return;
    }
    
    // Don't draw if no parts are selected
    if query.is_empty() {
        return;
    }
    
    for (global_transform, base_part) in &query {
        let transform = global_transform.compute_transform();
        let pos = transform.translation;
        let rot = transform.rotation;
        
        // Use BasePart.size if available (actual part size), otherwise fall back to transform.scale
        let size = base_part.map(|bp| bp.size).unwrap_or(transform.scale);
        
        // IMPROVED SCALING: Better proportional scaling for small objects
        let part_size = size.max_element();
        let handle_length = if part_size < 1.0 {
            // Small objects: scale more aggressively, minimum 0.2 instead of 0.4
            (part_size * 0.8).max(0.2)
        } else {
            // Large objects: normal scaling
            (part_size * 0.4) + 0.4
        };
        let cube_size = 0.35; // Larger cube size for easier visibility and grabbing
        
        // Get rotated local axes for proper handle alignment
        let local_x = rot * Vec3::X;
        let local_y = rot * Vec3::Y;
        let local_z = rot * Vec3::Z;
        
        // Position handles at the edges of the part using actual size AND rotation
        let start_x_pos = pos + local_x * (size.x * 0.5);
        let start_x_neg = pos - local_x * (size.x * 0.5);
        let start_y_pos = pos + local_y * (size.y * 0.5);  // Top of part
        let start_y_neg = pos - local_y * (size.y * 0.5);  // Bottom of part
        let start_z_pos = pos + local_z * (size.z * 0.5);
        let start_z_neg = pos - local_z * (size.z * 0.5);
        
        let highlight_x_pos = state.dragged_axis == Some(ScaleAxis::XPos);
        let highlight_x_neg = state.dragged_axis == Some(ScaleAxis::XNeg);
        let highlight_y_pos = state.dragged_axis == Some(ScaleAxis::YPos);
        let highlight_y_neg = state.dragged_axis == Some(ScaleAxis::YNeg);
        let highlight_z_pos = state.dragged_axis == Some(ScaleAxis::ZPos);
        let highlight_z_neg = state.dragged_axis == Some(ScaleAxis::ZNeg);
        
        // X axis (Red) - BOTH directions (using rotated local axis)
        let x_end_pos = start_x_pos + local_x * handle_length;
        let x_end_neg = start_x_neg - local_x * handle_length;
        gizmos.line(start_x_pos, x_end_pos, if highlight_x_pos { Color::srgb(1.0, 1.0, 0.0) } else { Color::srgb(1.0, 0.0, 0.0) });
        gizmos.cube(
            Transform::from_translation(x_end_pos).with_rotation(rot).with_scale(Vec3::splat(cube_size)),
            if highlight_x_pos { Color::srgb(1.0, 1.0, 0.0) } else { Color::srgb(1.0, 0.0, 0.0) }
        );
        gizmos.line(start_x_neg, x_end_neg, if highlight_x_neg { Color::srgb(1.0, 1.0, 0.0) } else { Color::srgb(1.0, 0.0, 0.0) });
        gizmos.cube(
            Transform::from_translation(x_end_neg).with_rotation(rot).with_scale(Vec3::splat(cube_size)),
            if highlight_x_neg { Color::srgb(1.0, 1.0, 0.0) } else { Color::srgb(1.0, 0.0, 0.0) }
        );
        
        // Y axis (Green) - BOTH directions (using rotated local axis)
        let y_end_pos = start_y_pos + local_y * handle_length;
        let y_end_neg = start_y_neg - local_y * handle_length;
        gizmos.line(start_y_pos, y_end_pos, if highlight_y_pos { Color::srgb(1.0, 1.0, 0.0) } else { Color::srgb(0.0, 1.0, 0.0) });
        gizmos.cube(
            Transform::from_translation(y_end_pos).with_rotation(rot).with_scale(Vec3::splat(cube_size)),
            if highlight_y_pos { Color::srgb(1.0, 1.0, 0.0) } else { Color::srgb(0.0, 1.0, 0.0) }
        );
        gizmos.line(start_y_neg, y_end_neg, if highlight_y_neg { Color::srgb(1.0, 1.0, 0.0) } else { Color::srgb(0.0, 1.0, 0.0) });
        gizmos.cube(
            Transform::from_translation(y_end_neg).with_rotation(rot).with_scale(Vec3::splat(cube_size)),
            if highlight_y_neg { Color::srgb(1.0, 1.0, 0.0) } else { Color::srgb(0.0, 1.0, 0.0) }
        );
        
        // Z axis (Blue) - BOTH directions (using rotated local axis)
        let z_end_pos = start_z_pos + local_z * handle_length;
        let z_end_neg = start_z_neg - local_z * handle_length;
        gizmos.line(start_z_pos, z_end_pos, if highlight_z_pos { Color::srgb(1.0, 1.0, 0.0) } else { Color::srgb(0.0, 0.0, 1.0) });
        gizmos.cube(
            Transform::from_translation(z_end_pos).with_rotation(rot).with_scale(Vec3::splat(cube_size)),
            if highlight_z_pos { Color::srgb(1.0, 1.0, 0.0) } else { Color::srgb(0.0, 0.0, 1.0) }
        );
        gizmos.line(start_z_neg, z_end_neg, if highlight_z_neg { Color::srgb(1.0, 1.0, 0.0) } else { Color::srgb(0.0, 0.0, 1.0) });
        gizmos.cube(
            Transform::from_translation(z_end_neg).with_rotation(rot).with_scale(Vec3::splat(cube_size)),
            if highlight_z_neg { Color::srgb(1.0, 1.0, 0.0) } else { Color::srgb(0.0, 0.0, 1.0) }
        );
        
        // No center cube - uniform scaling is done via corner handles or Shift+drag
        // The white cube was confusing and didn't match the visual style
    }
}

/// System to handle mouse interaction with scale gizmos
fn handle_scale_interaction(
    mut state: ResMut<ScaleToolState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut query: Query<(Entity, &GlobalTransform, &mut Transform, Option<&mut crate::classes::BasePart>, Option<&crate::classes::Part>, Option<&mut Mesh3d>), With<SelectionBox>>,
    editor_settings: Res<crate::editor_settings::EditorSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    // Query for parents to check hierarchy relationships (ChildOf in Bevy 0.17)
    parent_query: Query<&ChildOf>,
    // Undo stack for recording transform changes
    mut undo_stack: ResMut<crate::undo::UndoStack>,
) {
    if !state.active {
        return;
    }
    
    // TODO: Check Slint UI focus state to block input when UI has focus
    
    let Ok(window) = windows.single() else { return; };
    let Some(cursor_pos) = window.cursor_position() else { return; };
    
    let Ok((camera, camera_transform)) = cameras.single() else { return; };
    
    // Get camera forward direction for axis-aware drag calculations
    let camera_forward = camera_transform.forward();
    let camera_right = camera_transform.right();
    
    // Get ray from cursor
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else { return; };
    
    // Check if Ctrl is pressed for symmetric scaling
    let ctrl_pressed = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    
    if mouse.just_pressed(MouseButton::Left) {
        // First check if clicking on a scale handle
        let mut clicked_handle = false;
        for (entity, global_transform, transform, basepart_opt, _part_opt, _mesh_opt) in query.iter() {
            let t = global_transform.compute_transform();
            let pos = t.translation;
            let rot = t.rotation;
            
            // Use BasePart.size if available (actual part size)
            let size = basepart_opt.as_ref().map(|bp| bp.size).unwrap_or(Vec3::ONE);
            
            // IMPROVED SCALING: Use same logic as draw function
            let part_size = size.max_element();
            let handle_length = if part_size < 1.0 {
                // Small objects: scale more aggressively, minimum 0.2
                (part_size * 0.8).max(0.2)
            } else {
                // Large objects: normal scaling
                (part_size * 0.4) + 0.4
            };
            
            if let Some(axis) = detect_handle_hit_with_rotation(&ray, pos, rot, size, handle_length) {
                // Clicked on a handle - enter scale mode
                state.dragged_axis = Some(axis);
                state.initial_scale = size; // Store actual size, not transform.scale
                state.initial_position = transform.translation; // Store LOCAL position for restoring
                state.drag_start_pos = cursor_pos;
                state.dragged_entity = Some(entity);
                
                // Store initial scales and positions of ALL selected parts
                // Use BasePart.size for actual dimensions
                state.initial_scales.clear();
                state.initial_positions.clear();
                for (ent, _, trans, bp_opt, _, _) in query.iter() {
                    let ent_size = bp_opt.as_ref().map(|bp| bp.size).unwrap_or(Vec3::ONE);
                    state.initial_scales.insert(ent, ent_size);
                    state.initial_positions.insert(ent, trans.translation); // Local position
                }
                
                // Calculate initial world position
                if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, pos, Vec3::Y) {
                    state.initial_mouse_world = ray.origin + *ray.direction * t;
                }
                clicked_handle = true;
                break;
            }
        }
        
        // NOTE: If didn't click on a handle, Select Tool's base behavior handles drag-to-move
        // We don't duplicate that logic here to avoid jitter and inconsistency
    } else if mouse.pressed(MouseButton::Left) {
        if let Some(axis) = state.dragged_axis {
            // Calculate mouse delta in screen space
            let delta_screen = cursor_pos - state.drag_start_pos;
            
            // Progressive sensitivity: starts slow, increases linearly with drag distance
            // This gives fine control for small adjustments and faster scaling for large drags
            let drag_distance = delta_screen.length();
            let base_sensitivity = 0.015; // Lower base for finer control
            let progressive_factor = 1.0 + (drag_distance * 0.002); // Increases linearly with distance
            let sensitivity = base_sensitivity * progressive_factor;
            
            // Determine drag direction based on axis and screen movement
            // The key insight: we need to project the world axis onto screen space
            // and use the screen movement that best aligns with that projected axis
            // Y axis: always use screen Y (vertical is always vertical on screen)
            // X and Z axes: depend on camera orientation
            let drag_amount = match axis {
                ScaleAxis::YPos | ScaleAxis::YNeg => {
                    // Y axis: dragging up increases, dragging down decreases
                    -delta_screen.y * sensitivity
                }
                ScaleAxis::XPos | ScaleAxis::XNeg => {
                    // X axis: Use camera-relative calculation
                    // Project world X axis onto camera's right/forward plane
                    let x_dot_right = camera_right.dot(Vec3::X);
                    let x_dot_forward = camera_forward.dot(Vec3::X);
                    
                    // Use whichever screen axis (X or Y) best represents world X
                    if x_dot_right.abs() > x_dot_forward.abs() {
                        // World X aligns with screen X (camera right)
                        // Sign determines if dragging right = positive X
                        delta_screen.x * sensitivity * x_dot_right.signum()
                    } else {
                        // World X aligns with screen Y (camera forward/depth)
                        // This happens when looking along X axis
                        -delta_screen.y * sensitivity * x_dot_forward.signum()
                    }
                }
                ScaleAxis::ZPos | ScaleAxis::ZNeg => {
                    // Z axis: Use camera-relative calculation
                    // Project world Z axis onto camera's right/forward plane
                    let z_dot_right = camera_right.dot(Vec3::Z);
                    let z_dot_forward = camera_forward.dot(Vec3::Z);
                    
                    // Use whichever screen axis (X or Y) best represents world Z
                    if z_dot_right.abs() > z_dot_forward.abs() {
                        // World Z aligns with screen X (camera right)
                        delta_screen.x * sensitivity * z_dot_right.signum()
                    } else {
                        // World Z aligns with screen Y (camera forward/depth)
                        -delta_screen.y * sensitivity * z_dot_forward.signum()
                    }
                }
                ScaleAxis::Uniform => {
                    // Uniform: use diagonal movement (both X and Y contribute)
                    (delta_screen.x - delta_screen.y) * sensitivity * 0.5
                }
            };
            
            // Get the direction multiplier for the axis
            // Positive handles: dragging in positive direction increases size
            // Negative handles: dragging in positive direction also increases size (natural motion)
            // The key insight: when you grab a handle and drag OUTWARD from the object, it should grow
            let direction_mult = match axis {
                ScaleAxis::XPos | ScaleAxis::YPos | ScaleAxis::ZPos => 1.0,
                // For negative handles, we want dragging LEFT (negative screen X) to increase size
                // So we invert the drag amount
                ScaleAxis::XNeg => -1.0,
                ScaleAxis::YNeg => -1.0,  // Dragging DOWN increases size
                ScaleAxis::ZNeg => -1.0,  // Dragging "toward camera" increases size
                ScaleAxis::Uniform => 1.0,
            };
            
            let effective_drag = drag_amount * direction_mult;
            
            // Collect selected entities set for hierarchy check
            let selected_entities: std::collections::HashSet<Entity> = query.iter().map(|(e, ..)| e).collect();
            
            if ctrl_pressed {
                // CTRL HELD: Symmetric/center scaling (scale both sides equally, position stays centered)
                for (entity, _, mut transform, basepart_opt, part_opt, mesh_opt) in query.iter_mut() {
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

                    if let Some(initial_size) = state.initial_scales.get(&entity) {
                        if let Some(initial_pos) = state.initial_positions.get(&entity) {
                            // Calculate new size based on axis
                            let new_size = match axis {
                                ScaleAxis::XPos | ScaleAxis::XNeg => {
                                    let new_x = (initial_size.x + effective_drag).max(0.1);
                                    Vec3::new(new_x, initial_size.y, initial_size.z)
                                }
                                ScaleAxis::YPos | ScaleAxis::YNeg => {
                                    let new_y = (initial_size.y + effective_drag).max(0.1);
                                    Vec3::new(initial_size.x, new_y, initial_size.z)
                                }
                                ScaleAxis::ZPos | ScaleAxis::ZNeg => {
                                    let new_z = (initial_size.z + effective_drag).max(0.1);
                                    Vec3::new(initial_size.x, initial_size.y, new_z)
                                }
                                ScaleAxis::Uniform => {
                                    let scale_factor = (1.0 + effective_drag / initial_size.max_element()).max(0.1);
                                    *initial_size * scale_factor
                                }
                            };
                            
                            // Apply snapping if enabled
                            // Minimum size is fixed at 0.1, independent of snap grid
                            const MIN_PART_SIZE: f32 = 0.1;
                            let final_size = if editor_settings.snap_enabled {
                                let snap = editor_settings.snap_size;
                                Vec3::new(
                                    (new_size.x / snap).round() * snap,
                                    (new_size.y / snap).round() * snap,
                                    (new_size.z / snap).round() * snap,
                                ).max(Vec3::splat(MIN_PART_SIZE))
                            } else {
                                new_size
                            };
                            
                            // Update BasePart.size (source of truth)
                            if let Some(mut basepart) = basepart_opt {
                                basepart.size = final_size;
                            }
                            
                            // Regenerate mesh at new size
                            if let (Some(part), Some(mut mesh3d)) = (part_opt, mesh_opt) {
                                let new_mesh = match part.shape {
                                    crate::classes::PartType::Block => meshes.add(bevy::math::primitives::Cuboid::from_size(final_size)),
                                    crate::classes::PartType::Ball => meshes.add(bevy::math::primitives::Sphere::new(final_size.x / 2.0)),
                                    crate::classes::PartType::Cylinder => meshes.add(bevy::math::primitives::Cylinder::new(final_size.x / 2.0, final_size.y)),
                                    _ => meshes.add(bevy::math::primitives::Cuboid::from_size(final_size)),
                                };
                                mesh3d.0 = new_mesh;
                            }
                            
                            // Keep transform.scale at ONE, only update position
                            transform.scale = Vec3::ONE;
                            transform.translation = *initial_pos; // Position stays centered
                        }
                    }
                }
            } else {
                // NO CTRL: One-sided scaling (only the dragged face moves, opposite face stays fixed)
                // This properly handles rotation by transforming the offset to world space
                for (entity, _, mut transform, basepart_opt, part_opt, mesh_opt) in query.iter_mut() {
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

                    if let Some(initial_size) = state.initial_scales.get(&entity) {
                        if let Some(initial_pos) = state.initial_positions.get(&entity) {
                            let rot = transform.rotation;
                            
                            // Calculate new size based on axis
                            let new_size = match axis {
                                ScaleAxis::XPos | ScaleAxis::XNeg => {
                                    let new_x = (initial_size.x + effective_drag).max(0.1);
                                    Vec3::new(new_x, initial_size.y, initial_size.z)
                                }
                                ScaleAxis::YPos | ScaleAxis::YNeg => {
                                    let new_y = (initial_size.y + effective_drag).max(0.1);
                                    Vec3::new(initial_size.x, new_y, initial_size.z)
                                }
                                ScaleAxis::ZPos | ScaleAxis::ZNeg => {
                                    let new_z = (initial_size.z + effective_drag).max(0.1);
                                    Vec3::new(initial_size.x, initial_size.y, new_z)
                                }
                                ScaleAxis::Uniform => {
                                    let scale_factor = (1.0 + effective_drag / initial_size.max_element()).max(0.1);
                                    *initial_size * scale_factor
                                }
                            };
                            
                            // Apply snapping to the new size if enabled
                            // Minimum size is fixed at 0.1, independent of snap grid
                            const MIN_PART_SIZE: f32 = 0.1;
                            let final_size = if editor_settings.snap_enabled {
                                let snap = editor_settings.snap_size;
                                Vec3::new(
                                    (new_size.x / snap).round() * snap,
                                    (new_size.y / snap).round() * snap,
                                    (new_size.z / snap).round() * snap,
                                ).max(Vec3::splat(MIN_PART_SIZE))
                            } else {
                                new_size
                            };
                            
                            // Recalculate offset based on final (possibly snapped) size
                            let size_diff = final_size - *initial_size;
                            let final_local_offset = match axis {
                                ScaleAxis::XPos => Vec3::X * size_diff.x * 0.5,
                                ScaleAxis::XNeg => Vec3::NEG_X * size_diff.x * 0.5,
                                ScaleAxis::YPos => Vec3::Y * size_diff.y * 0.5,
                                ScaleAxis::YNeg => Vec3::NEG_Y * size_diff.y * 0.5,
                                ScaleAxis::ZPos => Vec3::Z * size_diff.z * 0.5,
                                ScaleAxis::ZNeg => Vec3::NEG_Z * size_diff.z * 0.5,
                                ScaleAxis::Uniform => Vec3::ZERO,
                            };
                            
                            // Transform offset to world space using part rotation
                            let world_offset = rot * final_local_offset;
                            
                            // Update BasePart.size (source of truth)
                            if let Some(mut basepart) = basepart_opt {
                                basepart.size = final_size;
                                basepart.cframe.translation = *initial_pos + world_offset;
                            }
                            
                            // Regenerate mesh at new size
                            if let (Some(part), Some(mut mesh3d)) = (part_opt, mesh_opt) {
                                let new_mesh = match part.shape {
                                    crate::classes::PartType::Block => meshes.add(bevy::math::primitives::Cuboid::from_size(final_size)),
                                    crate::classes::PartType::Ball => meshes.add(bevy::math::primitives::Sphere::new(final_size.x / 2.0)),
                                    crate::classes::PartType::Cylinder => meshes.add(bevy::math::primitives::Cylinder::new(final_size.x / 2.0, final_size.y)),
                                    _ => meshes.add(bevy::math::primitives::Cuboid::from_size(final_size)),
                                };
                                mesh3d.0 = new_mesh;
                            }
                            
                            // Keep transform.scale at ONE, only update position
                            transform.scale = Vec3::ONE;
                            transform.translation = *initial_pos + world_offset;
                        }
                    }
                }
            }
        }
    } else if mouse.just_released(MouseButton::Left) {
        // Record undo action if we were scaling
        if state.dragged_axis.is_some() && !state.initial_scales.is_empty() {
            let mut old_states: Vec<(u64, [f32; 3], [f32; 3])> = Vec::new();
            let mut new_states: Vec<(u64, [f32; 3], [f32; 3])> = Vec::new();
            
            for (entity, _, transform, basepart_opt, _, _) in query.iter() {
                if let Some(initial_pos) = state.initial_positions.get(&entity) {
                    if let Some(initial_size) = state.initial_scales.get(&entity) {
                        // Check if position changed (scale changes position for asymmetric scaling)
                        let pos_changed = (*initial_pos - transform.translation).length() > 0.001;
                        
                        // Check if size changed via BasePart
                        let size_changed = if let Some(bp) = &basepart_opt {
                            (*initial_size - bp.size).length() > 0.001
                        } else {
                            false
                        };
                        
                        if pos_changed || size_changed {
                            let new_size = basepart_opt.as_ref().map(|bp| bp.size).unwrap_or(*initial_size);
                            old_states.push((
                                entity.to_bits(),
                                initial_pos.to_array(),
                                initial_size.to_array(),
                            ));
                            new_states.push((
                                entity.to_bits(),
                                transform.translation.to_array(),
                                new_size.to_array(),
                            ));
                        }
                    }
                }
            }
            
            // Push to undo stack if there were actual changes
            if !old_states.is_empty() {
                undo_stack.push(crate::undo::Action::ScaleEntities {
                    old_states,
                    new_states,
                });
            }
        }
        
        state.dragged_axis = None;
        state.dragged_entity = None;
        state.initial_scales.clear();
        state.initial_positions.clear();
    }
}

/// Public function to check if ray hits any scale handle (for selection system)
pub fn is_clicking_scale_handle(
    ray: &Ray3d,
    pos: Vec3,
    rot: Quat,
    scale: Vec3,
    handle_length: f32,
) -> bool {
    detect_handle_hit_with_rotation(ray, pos, rot, scale, handle_length).is_some()
}

/// Detect if ray hits a scale handle (with rotation support)
fn detect_handle_hit_with_rotation(ray: &Ray3d, pos: Vec3, rot: Quat, scale: Vec3, length: f32) -> Option<ScaleAxis> {
    // Hit radius for handle cubes - generous for easy clicking
    // Visual cube is 0.25, but hit area is larger for easier grabbing
    let cube_size = 0.6; // Larger hit radius for easier grabbing
    
    // Get rotated local axes for proper handle alignment
    let local_x = rot * Vec3::X;
    let local_y = rot * Vec3::Y;
    let local_z = rot * Vec3::Z;
    
    // Position handles at the edges of the part (same as draw function) - WITH ROTATION
    let start_x_pos = pos + local_x * (scale.x * 0.5);
    let start_x_neg = pos - local_x * (scale.x * 0.5);
    let start_y_pos = pos + local_y * (scale.y * 0.5);
    let start_y_neg = pos - local_y * (scale.y * 0.5);
    let start_z_pos = pos + local_z * (scale.z * 0.5);
    let start_z_neg = pos - local_z * (scale.z * 0.5);
    
    // Calculate handle end positions - WITH ROTATION
    let x_cube_pos = start_x_pos + local_x * length;
    let x_cube_neg = start_x_neg - local_x * length;
    let y_cube_pos = start_y_pos + local_y * length;
    let y_cube_neg = start_y_neg - local_y * length;
    let z_cube_pos = start_z_pos + local_z * length;
    let z_cube_neg = start_z_neg - local_z * length;
    
    // Check axis handles FIRST (before center) - they should have priority
    // Check X axis cubes (BOTH directions)
    if ray_to_point_distance(ray.origin, *ray.direction, x_cube_pos) < cube_size {
        return Some(ScaleAxis::XPos);
    }
    if ray_to_point_distance(ray.origin, *ray.direction, x_cube_neg) < cube_size {
        return Some(ScaleAxis::XNeg);
    }
    
    // Check Y axis cubes (BOTH directions)
    if ray_to_point_distance(ray.origin, *ray.direction, y_cube_pos) < cube_size {
        return Some(ScaleAxis::YPos);
    }
    if ray_to_point_distance(ray.origin, *ray.direction, y_cube_neg) < cube_size {
        return Some(ScaleAxis::YNeg);
    }
    
    // Check Z axis cubes (BOTH directions)
    if ray_to_point_distance(ray.origin, *ray.direction, z_cube_pos) < cube_size {
        return Some(ScaleAxis::ZPos);
    }
    if ray_to_point_distance(ray.origin, *ray.direction, z_cube_neg) < cube_size {
        return Some(ScaleAxis::ZNeg);
    }
    
    // No center cube for uniform scaling - removed for cleaner UX
    None
}

