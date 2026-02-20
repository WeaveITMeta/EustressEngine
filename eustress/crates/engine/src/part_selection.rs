use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::rendering::PartEntity;
use crate::classes::{Instance, BasePart};
use crate::selection_box::SelectionBox;
use crate::math_utils::ray_obb_intersection;
use crate::entity_utils::entity_to_id_string;

#[cfg(not(target_arch = "wasm32"))]
use crate::rendering::BevySelectionManager;

/// System for left-click part selection with raycasting (Modern ECS)
/// Supports both PartEntity (legacy) and Instance (modern) components
#[cfg(not(target_arch = "wasm32"))]
pub fn part_selection_system(
    mouse_button: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    // Query parts with PartEntity OR Instance component (either works for selection)
    part_entities_query: Query<(Entity, Option<&PartEntity>, Option<&Instance>, &GlobalTransform, &Mesh3d, Option<&BasePart>, Option<&ChildOf>)>,
    // Query for children to calculate accurate group bounds (matching move_tool.rs)
    children_query: Query<&Children>,
    // Query for child transforms/baseparts
    child_transform_query: Query<(&GlobalTransform, Option<&BasePart>)>,
    // Query for SELECTED entities (for tool handle checks) - Matches tool rendering logic
    selected_query: Query<(Entity, &GlobalTransform, Option<&BasePart>), With<SelectionBox>>,
    // Query to check if a parent entity is a Model
    parent_query: Query<&Instance>,
    selection_manager: Option<Res<BevySelectionManager>>,
    move_state: Res<crate::move_tool::MoveToolState>,
    scale_state: Res<crate::scale_tool::ScaleToolState>,
    rotate_state: Res<crate::rotate_tool::RotateToolState>,
    _studio_state: Res<crate::ui::StudioState>,
) {
    let Some(selection_manager) = selection_manager else { return };
    // Only trigger on left click press
    if !mouse_button.just_pressed(MouseButton::Left) {
        return;
    }
    
    // TODO: Check Slint UI focus state to block input when UI has focus
    
    // Check if Shift or Ctrl is pressed for multi-select
    let shift_pressed = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let ctrl_pressed = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let alt_pressed = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let multi_select_modifier = shift_pressed || ctrl_pressed;
    
    let window = match windows.single() {
        Ok(w) => w,
        Err(_) => return,
    };
    
    let cursor_position = match window.cursor_position() {
        Some(pos) => pos,
        None => return,
    };
    
    let (camera, camera_transform) = match camera_query.single() {
        Ok(ct) => ct,
        Err(_) => return,
    };
    
    // Convert screen space to ray
    let ray = match camera.viewport_to_world(camera_transform, cursor_position) {
        Ok(r) => r,
        Err(_) => return,
    };
    
    // Raycast against all part entities
    // Tuple: (part_id, distance, entity, parent_entity_if_model)
    let mut closest_hit: Option<(String, f32, Entity, Option<Entity>)> = None;
    
    // PRIORITY CHECK: Check if we are clicking on a tool handle BEFORE checking for part hits
    // This ensures handles are always clickable even if a part is behind them
    
    // Check Move Tool handles
    if move_state.active {
        if !selected_query.is_empty() {
            let mut bounds_min = Vec3::splat(f32::MAX);
            let mut bounds_max = Vec3::splat(f32::MIN);
            let mut count = 0;
            
            for (entity, global_transform, basepart) in selected_query.iter() {
                let t = global_transform.compute_transform();
                
                // Use calculated AABB for accurate center (matches MoveTool logic)
                let size = basepart.map(|bp| bp.size).unwrap_or(t.scale);
                let half_size = size * 0.5;
                let (part_min, part_max) = crate::math_utils::calculate_rotated_aabb(t.translation, half_size, t.rotation);
                
                bounds_min = bounds_min.min(part_min);
                bounds_max = bounds_max.max(part_max);
                count += 1;
                
                // Include children in bounds calculation (CRITICAL for Models)
                if let Ok(children) = children_query.get(entity) {
                    for child in children.iter() {
                        if let Ok((child_global, child_bp)) = child_transform_query.get(child) {
                            let child_t = child_global.compute_transform();
                            let child_size = child_bp.map(|bp| bp.size).unwrap_or(child_t.scale);
                            let child_half = child_size * 0.5;
                            let (c_min, c_max) = crate::math_utils::calculate_rotated_aabb(child_t.translation, child_half, child_t.rotation);
                            
                            bounds_min = bounds_min.min(c_min);
                            bounds_max = bounds_max.max(c_max);
                            count += 1;
                        }
                    }
                }
            }
            
            if count > 0 {
                let center = (bounds_min + bounds_max) * 0.5;
                let selection_size = bounds_max - bounds_min;
                let avg_size = selection_size.max_element();
                
                // Match handle length from move_tool.rs
                let handle_length = (avg_size * 0.6).max(1.0) + 1.5;
                
                if crate::move_tool::is_clicking_move_handle(&ray, center, Vec3::splat(avg_size), handle_length, &camera_transform) {
                    return; // Clicking move handle, abort selection
                }
            }
        }
    }
    
    // Check Rotate Tool handles
    if rotate_state.active {
        for (_entity, global_transform, basepart) in selected_query.iter() {
            let t = global_transform.compute_transform();
            
            // Match radius calculation from rotate_tool.rs
            let part_size = if let Some(bp) = basepart {
                bp.size.max_element()
            } else {
                t.scale.max_element()
            };
            let radius = (part_size * 0.6).max(2.0).min(50.0);
            
            if crate::rotate_tool::is_clicking_rotate_handle(&ray, t.translation, radius, &camera_transform) {
                return; // Clicking rotate handle, abort selection
            }
        }
    }
    
    // Check Scale Tool handles
    if scale_state.active {
        for (_entity, global_transform, basepart) in selected_query.iter() {
            let t = global_transform.compute_transform();
            
            // Match handle logic from scale_tool.rs
            let part_size = if let Some(bp) = basepart {
                bp.size
            } else {
                t.scale
            };
            
            let handle_length = (part_size.max_element() * 0.4) + 0.4;
            
            if crate::scale_tool::is_clicking_scale_handle(&ray, t.translation, t.rotation, part_size, handle_length) {
                return; // Clicking scale handle, abort selection
            }
        }
    }
    
    for (entity, part_entity, instance, transform, _mesh_handle, basepart, child_of) in part_entities_query.iter() {
        // Skip entities that don't have either PartEntity or Instance (not selectable)
        // Entity ID format must match: "indexVgeneration" e.g. "68v0"
        let entity_id = entity_to_id_string(entity);
        
        let part_id = if let Some(pe) = part_entity {
            if !pe.part_id.is_empty() {
                pe.part_id.clone()
            } else if instance.is_some() {
                entity_id.clone()
            } else {
                continue;
            }
        } else if instance.is_some() {
            entity_id.clone()
        } else {
            continue; // No identifier, skip
        };
        
        // Skip locked parts - they cannot be selected!
        if let Some(bp) = basepart {
            if bp.locked {
                continue;
            }
        }
        
        // Get part transform
        let part_transform = transform.compute_transform();
        let part_position = part_transform.translation;
        let part_rotation = part_transform.rotation;
        
        // Use BasePart.size if available, otherwise fall back to transform scale
        let part_size = basepart.map(|bp| bp.size).unwrap_or(part_transform.scale);
        
        // Use precise OBB (Oriented Bounding Box) intersection
        if let Some(distance) = ray_obb_intersection(ray.origin, *ray.direction, part_position, part_size, part_rotation) {
            // Check if this entity has a parent that is a Model
            let parent_model = child_of.and_then(|c| {
                let parent_entity = c.parent();
                // Check if parent is a Model
                if let Ok(parent_instance) = parent_query.get(parent_entity) {
                    if parent_instance.class_name == crate::classes::ClassName::Model {
                        return Some(parent_entity);
                    }
                }
                None
            });
            
            // Keep track of closest hit
            if closest_hit.is_none() || distance < closest_hit.as_ref().unwrap().1 {
                closest_hit = Some((part_id, distance, entity, parent_model));
            }
        }
    }
    
    // Update selection
    if let Some((part_id, _, _hit_entity, parent_model)) = closest_hit {
        // Hit part: part_id
        
        // Hit a part - check if we should allow selection changes
        // Only block if a tool is ACTIVELY DRAGGING (not just active/visible)
        let tool_is_dragging = 
            (move_state.active && move_state.dragged_axis.is_some()) ||
            (scale_state.active && scale_state.dragged_axis.is_some()) ||
            (rotate_state.active && rotate_state.dragged_axis.is_some());
        
        if tool_is_dragging {
            return; // Tool is being used right now, don't change selection
        }
        
        // Determine what to select: the part itself, or its parent Model
        // Alt+Click = select the individual part (bypass parent selection)
        // Normal Click = select the parent Model if the part is a child of one
        let selection_id = if alt_pressed {
            // Alt held: select the individual part directly
            // Alt+Click: Selecting individual part
            part_id.clone()
        } else if let Some(model_entity) = parent_model {
            // Part has a parent Model - select the Model instead
            let model_id = entity_to_id_string(model_entity);
            // Selecting parent Model
            model_id
        } else {
            // No parent Model - select the part itself
            part_id.clone()
        };
        
        let sel = selection_manager.0.write();
        
        if multi_select_modifier {
            // Shift+Click or Ctrl+Click: Toggle part in selection (multi-select)
            // Works even when tools are active!
            if sel.is_selected(&selection_id) {
                sel.remove_from_selection(&selection_id);
                // Removed from selection
            } else {
                sel.add_to_selection(selection_id.clone());
                // Added to selection
            }
        } else {
            // Normal click: Replace selection
            // Only if no tool is being actively used
            sel.select(selection_id.clone());
            // Selected
        }
    } else {
        // Clicked on empty space - check if we should deselect
        // Block deselection if:
        // 1. Actively dragging a tool handle
        // 2. About to click a tool handle (prevent clearing before tool processes click)
        let tool_is_dragging = 
            (move_state.active && move_state.dragged_axis.is_some()) ||
            (scale_state.active && scale_state.dragged_axis.is_some()) ||
            (rotate_state.active && rotate_state.dragged_axis.is_some());
        
        if tool_is_dragging {
            return; // Tool is being used, don't deselect
        }
        
        // Check if we're about to click a tool handle
        // For move tool: check group center
        if move_state.active {
            let sel = selection_manager.0.read();
            let selected = sel.get_selected();
            
            if !selected.is_empty() {
                let mut center = Vec3::ZERO;
                let mut total_scale = 0.0;
                let mut count = 0;
                
                for (entity, part_entity, instance, transform, _mesh, _basepart, _child_of) in part_entities_query.iter() {
                    // Get part ID from either component (format: "indexVgeneration")
                    let entity_id = entity_to_id_string(entity);
                    let part_id = part_entity.map(|pe| pe.part_id.clone())
                        .filter(|id| !id.is_empty())
                        .or_else(|| instance.map(|_| entity_id));
                    
                    if let Some(id) = part_id {
                        if selected.contains(&id) {
                            let t = transform.compute_transform();
                            center += t.translation;
                            total_scale += t.scale.max_element();
                            count += 1;
                        }
                    }
                }
                
                if count > 0 {
                    center /= count as f32;
                    let avg_scale = total_scale / count as f32;
                    let handle_length = (avg_scale * 0.5) + 1.5;
                    
                    if crate::move_tool::is_clicking_move_handle(&ray, center, Vec3::splat(avg_scale), handle_length, &camera_transform) {
                        return; // About to click move handle, don't clear selection
                    }
                }
            }
        }
        
        // For scale tool: check each selected part
        if scale_state.active {
            let sel = selection_manager.0.read();
            let selected = sel.get_selected();
            
            for (entity, part_entity, instance, transform, _mesh, _basepart, _child_of) in part_entities_query.iter() {
                // Get part ID from either component (format: "indexVgeneration")
                let entity_id = entity_to_id_string(entity);
                let part_id = part_entity.map(|pe| pe.part_id.clone())
                    .filter(|id| !id.is_empty())
                    .or_else(|| instance.map(|_| entity_id));
                
                if let Some(id) = part_id {
                    if selected.contains(&id) {
                        let t = transform.compute_transform();
                        let part_size = t.scale.max_element();
                        let handle_length = (part_size * 0.5) + 0.5;
                        
                        if crate::scale_tool::is_clicking_scale_handle(&ray, t.translation, t.rotation, t.scale, handle_length) {
                            return; // About to click scale handle, don't clear selection
                        }
                    }
                }
            }
        }
        
        // Clear selection when clicking on empty space (no part hit)
        // This works for ALL tools - clicking on nothing should deselect
        let sel = selection_manager.0.write();
        sel.clear();
        info!("Deselected - clicked on empty space");
    }
}

