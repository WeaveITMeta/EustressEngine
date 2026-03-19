use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::rendering::PartEntity;
use eustress_common::default_scene::PartEntityMarker;
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
    camera_query: Query<(&Camera, &GlobalTransform, &Projection)>,
    // Query selectable parts: must have PartEntityMarker OR (Instance + BasePart) so Folders,
    // Services, Scripts, and UI entities are excluded from raycasting entirely.
    part_entities_query: Query<(Entity, Option<&PartEntity>, Option<&PartEntityMarker>, Option<&Instance>, &GlobalTransform, Option<&Mesh3d>, Option<&BasePart>, Option<&ChildOf>),
        Or<(With<PartEntityMarker>, With<PartEntity>, (With<BasePart>, With<Instance>))>>,
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
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
) {
    let Some(selection_manager) = selection_manager else { return };
    // Only trigger on left click press
    if !mouse_button.just_pressed(MouseButton::Left) {
        return;
    }
    
    // Check if click is within the viewport bounds (not on UI panels)
    let window = match windows.single() {
        Ok(w) => w,
        Err(_) => return,
    };
    
    let cursor_position = match window.cursor_position() {
        Some(pos) => pos,
        None => return,
    };
    
    // Block selection if click is outside the 3D viewport area
    if let Some(vb) = viewport_bounds.as_ref() {
        if vb.width > 0.0 && vb.height > 0.0 {
            let in_viewport = cursor_position.x >= vb.x 
                && cursor_position.x <= vb.x + vb.width
                && cursor_position.y >= vb.y 
                && cursor_position.y <= vb.y + vb.height;
            if !in_viewport {
                return; // Click is on UI, not viewport
            }
        }
    }
    
    // Check if Shift or Ctrl is pressed for multi-select
    let shift_pressed = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let ctrl_pressed = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let alt_pressed = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let multi_select_modifier = shift_pressed || ctrl_pressed;
    
    // Find the main 3D camera (order=0) — there may be multiple cameras (e.g. Slint overlay at order=100)
    let (camera, camera_transform, projection) = match camera_query.iter().find(|(c, _, _)| c.order == 0) {
        Some(ct) => ct,
        None => return,
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
                
                // MUST match move_tool.rs camera_scale_factor exactly!
                let fov = match projection {
                    Projection::Perspective(p) => p.fov,
                    _ => std::f32::consts::FRAC_PI_4,
                };
                let cam_dist = (center - camera_transform.translation()).length().max(0.1);
                let scale = cam_dist * (fov * 0.5).tan() * 0.16;
                let handle_length = scale * 1.0;
                
                if crate::move_tool::is_clicking_move_handle(&ray, center, Vec3::ONE, handle_length, &camera_transform) {
                    return; // Clicking move handle, abort selection
                }
            }
        }
    }
    
    // Check Rotate Tool handles
    if rotate_state.active {
        // Compute combined bounding box of all selected entities (matching rotate_tool.rs)
        let mut rot_bmin = Vec3::splat(f32::MAX);
        let mut rot_bmax = Vec3::splat(f32::MIN);
        let mut rot_count = 0;
        for (entity, global_transform, basepart) in selected_query.iter() {
            let t = global_transform.compute_transform();
            let size = basepart.map(|bp| bp.size).unwrap_or(t.scale);
            let (mn, mx) = crate::math_utils::calculate_rotated_aabb(t.translation, size * 0.5, t.rotation);
            rot_bmin = rot_bmin.min(mn);
            rot_bmax = rot_bmax.max(mx);
            rot_count += 1;
            
            // Include children in bounds (matching rotate_tool.rs)
            if let Ok(children) = children_query.get(entity) {
                for child in children.iter() {
                    if let Ok((child_global, child_bp)) = child_transform_query.get(child) {
                        let child_t = child_global.compute_transform();
                        let child_size = child_bp.map(|bp| bp.size).unwrap_or(child_t.scale);
                        let (c_min, c_max) = crate::math_utils::calculate_rotated_aabb(child_t.translation, child_size * 0.5, child_t.rotation);
                        rot_bmin = rot_bmin.min(c_min);
                        rot_bmax = rot_bmax.max(c_max);
                        rot_count += 1;
                    }
                }
            }
        }
        if rot_count > 0 {
            let rot_center = (rot_bmin + rot_bmax) * 0.5;
            let rot_extent = rot_bmax - rot_bmin;
            // Use same radius calculation as rotate_tool.rs
            let radius = crate::rotate_tool::compute_ring_radius(rot_center, rot_extent, &camera_transform, projection);
            if crate::rotate_tool::is_clicking_rotate_handle(&ray, rot_center, radius, &camera_transform) {
                return; // Clicking rotate handle, abort selection
            }
        }
    }
    
    // Check Scale Tool handles
    if scale_state.active {
        for (_entity, global_transform, basepart) in selected_query.iter() {
            let t = global_transform.compute_transform();
            
            let part_size = if let Some(bp) = basepart {
                bp.size
            } else {
                t.scale
            };
            
            // MUST match scale_tool.rs camera-distance-based handle length
            let fov_s = match projection {
                Projection::Perspective(p) => p.fov,
                _ => std::f32::consts::FRAC_PI_4,
            };
            let dist_s = (t.translation - camera_transform.translation()).length().max(0.1);
            let scale_s = dist_s * (fov_s * 0.5).tan() * 0.16;
            let handle_length = scale_s * 0.9;
            
            if crate::scale_tool::is_clicking_scale_handle(&ray, t.translation, t.rotation, part_size, handle_length) {
                return; // Clicking scale handle, abort selection
            }
        }
    }
    
    for (entity, part_entity, part_entity_marker, instance, transform, _mesh_handle, basepart, child_of) in part_entities_query.iter() {
        // Skip entities that don't have PartEntity, PartEntityMarker, or Instance (not selectable)
        // Entity ID format must match: "indexVgeneration" e.g. "68v0"
        let entity_id = entity_to_id_string(entity);
        
        // Skip non-Part class names — Folder, ScreenGui, Service, Script etc. are not selectable
        // even if they have an Instance component. Only Part/MeshPart/BasePart-carrying classes
        // should receive 3D click selection.
        if let Some(inst) = instance {
            match inst.class_name {
                crate::classes::ClassName::Folder
                | crate::classes::ClassName::Model
                | crate::classes::ClassName::ScreenGui
                | crate::classes::ClassName::Frame
                | crate::classes::ClassName::SoulScript
                | crate::classes::ClassName::Workspace
                | crate::classes::ClassName::Lighting
                | crate::classes::ClassName::Camera => continue,
                _ => {}
            }
        }

        let part_id = if let Some(pe) = part_entity {
            if !pe.part_id.is_empty() {
                pe.part_id.clone()
            } else if instance.is_some() {
                entity_id.clone()
            } else {
                continue;
            }
        } else if let Some(pem) = part_entity_marker {
            if !pem.part_id.is_empty() {
                pem.part_id.clone()
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
            if closest_hit.as_ref().map_or(true, |(_, d, _, _)| distance < *d) {
                closest_hit = Some((part_id, distance, entity, parent_model));
            }
        }
    }
    
    // Update selection
    if let Some((part_id, distance, _hit_entity, parent_model)) = closest_hit {
        info!("[select] hit part_id='{}' dist={:.2}", part_id, distance);
        
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
            part_id.clone()
        } else if let Some(model_entity) = parent_model {
            let model_id = entity_to_id_string(model_entity);
            info!("[select] selecting parent Model id='{}'", model_id);
            model_id
        } else {
            part_id.clone()
        };
        
        let sel = selection_manager.0.write();
        
        if multi_select_modifier {
            if sel.is_selected(&selection_id) {
                sel.remove_from_selection(&selection_id);
                info!("[select] removed '{}' from selection", selection_id);
            } else {
                sel.add_to_selection(selection_id.clone());
                info!("[select] added '{}' to selection", selection_id);
            }
        } else {
            sel.select(selection_id.clone());
            info!("[select] selected '{}'", selection_id);
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
                
                for (entity, part_entity, part_entity_marker, instance, transform, _mesh, _basepart, _child_of) in part_entities_query.iter() {
                    // Get part ID from either component (format: "indexVgeneration")
                    let entity_id = entity_to_id_string(entity);
                    let part_id = part_entity.map(|pe| pe.part_id.clone())
                        .filter(|id| !id.is_empty())
                        .or_else(|| part_entity_marker.map(|pem| pem.part_id.clone()).filter(|id| !id.is_empty()))
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
            
            for (entity, part_entity, part_entity_marker, instance, transform, _mesh, _basepart, _child_of) in part_entities_query.iter() {
                // Get part ID from either component (format: "indexVgeneration")
                let entity_id = entity_to_id_string(entity);
                let part_id = part_entity.map(|pe| pe.part_id.clone())
                    .filter(|id| !id.is_empty())
                    .or_else(|| part_entity_marker.map(|pem| pem.part_id.clone()).filter(|id| !id.is_empty()))
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

