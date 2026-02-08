#![allow(dead_code)]

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
#[allow(unused_imports)]
use bevy::gizmos::config::{GizmoConfigStore, GizmoConfigGroup, DefaultGizmoConfigGroup};

use crate::selection_box::SelectionBox;
use crate::gizmo_tools::TransformGizmoGroup;
use crate::math_utils::ray_plane_intersection;

/// Resource tracking the rotate tool state
#[derive(Resource, Default)]
pub struct RotateToolState {
    pub active: bool,
    pub dragged_axis: Option<Axis3d>,
    pub initial_rotation: Quat,  // For single entity (backward compat)
    pub drag_start_angle: f32,
    pub drag_start_pos: Vec2,
    pub initial_rotations: std::collections::HashMap<Entity, Quat>,  // For multi-select
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

/// Plugin for the rotate tool functionality
pub struct RotateToolPlugin;

impl Plugin for RotateToolPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<RotateToolState>()
            .add_systems(Update, (
                draw_rotate_gizmos,
                handle_rotate_interaction,
            ));
    }
}

/// System to draw rotation circle gizmos for selected entities
fn draw_rotate_gizmos(
    mut gizmos: Gizmos<TransformGizmoGroup>,
    state: Res<RotateToolState>,
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
        
        // Scale radius based on part size (use BasePart.size if available)
        let part_size = if let Some(bp) = base_part {
            bp.size.max_element()
        } else {
            transform.scale.max_element()
        };
        
        // IMPROVED SCALING: Better proportional scaling for small objects
        let radius = if part_size < 1.0 {
            // Small objects: scale more aggressively, minimum 0.5 instead of 2.0
            (part_size * 1.5).max(0.5).min(50.0)
        } else {
            // Large objects: normal scaling, minimum 2.0
            (part_size * 0.6).max(2.0).min(50.0)
        };
        let segments = 64;
        
        // X axis rotation circle (Red - YZ plane)
        draw_rotation_circle(&mut gizmos, pos, Vec3::X, radius, segments, Color::srgb(1.0, 0.0, 0.0),
            state.dragged_axis == Some(Axis3d::X));
        
        // Y axis rotation circle (Green - XZ plane)
        draw_rotation_circle(&mut gizmos, pos, Vec3::Y, radius, segments, Color::srgb(0.0, 1.0, 0.0),
            state.dragged_axis == Some(Axis3d::Y));
        
        // Z axis rotation circle (Blue - XY plane)
        draw_rotation_circle(&mut gizmos, pos, Vec3::Z, radius, segments, Color::srgb(0.0, 0.0, 1.0),
            state.dragged_axis == Some(Axis3d::Z));
    }
}

/// Helper to draw a rotation circle around an axis
fn draw_rotation_circle(
    gizmos: &mut Gizmos<TransformGizmoGroup>,
    center: Vec3,
    axis: Vec3,
    radius: f32,
    segments: usize,
    color: Color,
    highlight: bool,
) {
    // Add transparency to circles for better visibility (50% alpha, full when highlighted)
    let color = if highlight {
        Color::srgba(1.0, 1.0, 0.0, 1.0) // Yellow when dragging (full opacity)
    } else {
        // Extract RGB and add alpha
        let rgba = color.to_srgba();
        Color::srgba(rgba.red, rgba.green, rgba.blue, 0.6) // 60% opacity
    };
    
    // Create perpendicular vectors for the circle plane
    let up = if axis == Vec3::Y || axis == -Vec3::Y {
        Vec3::X
    } else {
        Vec3::Y
    };
    
    let tangent1 = axis.cross(up).normalize();
    let tangent2 = axis.cross(tangent1).normalize();
    
    // Draw multiple circles for thickness (easier to see and click)
    let thickness_offsets = [0.0, 0.02, -0.02]; // Main + inner/outer for visual thickness
    
    for offset_factor in thickness_offsets {
        let r = radius * (1.0 + offset_factor);
        
        for i in 0..segments {
            let angle1 = (i as f32 / segments as f32) * std::f32::consts::TAU;
            let angle2 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
            
            let p1 = center + tangent1 * angle1.cos() * r + tangent2 * angle1.sin() * r;
            let p2 = center + tangent1 * angle2.cos() * r + tangent2 * angle2.sin() * r;
            
            gizmos.line(p1, p2, color);
        }
    }
}

/// System to handle mouse interaction with rotate gizmos
fn handle_rotate_interaction(
    mut state: ResMut<RotateToolState>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut query: Query<(Entity, &GlobalTransform, &mut Transform, Option<&mut crate::classes::BasePart>), With<SelectionBox>>,
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
    
    // Get ray from cursor
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else { return; };
    
    if mouse.just_pressed(MouseButton::Left) {
        // Check if clicking on any rotation circle
        for (entity, global_transform, transform, base_part) in query.iter_mut() {
            let t = global_transform.compute_transform();
            let pos = t.translation;
            
            // Scale radius based on part size (same as draw function)
            let part_size = if let Some(bp) = base_part {
                bp.size.max_element()
            } else {
                t.scale.max_element()
            };
            
            // IMPROVED SCALING: Use same logic as draw function
            let radius = if part_size < 1.0 {
                // Small objects: scale more aggressively, minimum 0.5
                (part_size * 1.5).max(0.5).min(50.0)
            } else {
                // Large objects: normal scaling, minimum 2.0
                (part_size * 0.6).max(2.0).min(50.0)
            };
            
            if let Some(axis) = detect_circle_hit(&ray, pos, radius, camera_transform) {
                // Start rotation - store initial rotations for ALL selected entities
                state.dragged_axis = Some(axis);
                state.initial_rotation = transform.rotation;
                state.drag_start_angle = calculate_rotation_angle(&ray, pos, axis);
                state.drag_start_pos = cursor_pos;
                
                // Store initial rotations of ALL selected parts
                state.initial_rotations.clear();
                for (ent, _, trans, _) in query.iter() {
                    state.initial_rotations.insert(ent, trans.rotation);
                }
                break;
            }
        }
    } else if mouse.pressed(MouseButton::Left) {
        if let Some(axis) = state.dragged_axis {
            // Get first entity position for angle calculation
            let first_pos = query.iter().next().map(|(_, gt, _, _)| gt.translation());
            
            if let Some(pos) = first_pos {
                // Calculate current angle
                let current_angle = calculate_rotation_angle(&ray, pos, axis);
                let delta_angle = current_angle - state.drag_start_angle;
                
                // Apply snapping (15 degree increments)
                let snapped_delta = snap_angle(delta_angle, 15.0_f32.to_radians());
                
                // Create rotation quaternion around the world axis
                let rotation_delta = Quat::from_axis_angle(axis.to_vec3(), snapped_delta);
                
                // Collect selected entities set for hierarchy check
                let selected_entities: std::collections::HashSet<Entity> = query.iter().map(|(e, ..)| e).collect();

                // Apply rotation to ALL selected entities
                for (entity, _, mut transform, basepart_opt) in query.iter_mut() {
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

                    if let Some(initial_rotation) = state.initial_rotations.get(&entity) {
                        // Apply the rotation delta to each entity's initial rotation
                        transform.rotation = rotation_delta * *initial_rotation;
                        
                        // Update BasePart.cframe.rotation to match Transform.rotation
                        if let Some(mut basepart) = basepart_opt {
                            basepart.cframe.rotation = transform.rotation;
                        }
                    }
                }
            }
        }
    } else if mouse.just_released(MouseButton::Left) {
        // Record undo action if we were dragging
        if state.dragged_axis.is_some() && !state.initial_rotations.is_empty() {
            let mut old_transforms: Vec<(u64, [f32; 3], [f32; 4])> = Vec::new();
            let mut new_transforms: Vec<(u64, [f32; 3], [f32; 4])> = Vec::new();
            
            for (entity, global_transform, transform, _) in query.iter() {
                if let Some(initial_rot) = state.initial_rotations.get(&entity) {
                    // Only record if rotation actually changed
                    let rot_changed = initial_rot.angle_between(transform.rotation) > 0.001;
                    
                    if rot_changed {
                        let pos = global_transform.translation();
                        old_transforms.push((
                            entity.to_bits(),
                            pos.to_array(),
                            initial_rot.to_array(),
                        ));
                        new_transforms.push((
                            entity.to_bits(),
                            pos.to_array(),
                            transform.rotation.to_array(),
                        ));
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
        state.initial_rotations.clear();
    }
}

/// Public function to check if ray hits any rotation handle (for selection system)
pub fn is_clicking_rotate_handle(
    ray: &Ray3d,
    pos: Vec3,
    radius: f32,
    camera_transform: &GlobalTransform,
) -> bool {
    detect_circle_hit(ray, pos, radius, camera_transform).is_some()
}

/// Detect if ray hits a rotation circle - finds the BEST matching axis
/// Uses EXTREMELY generous hit detection - if you're anywhere near a ring, it works
fn detect_circle_hit(
    ray: &Ray3d,
    center: Vec3,
    radius: f32,
    _camera_transform: &GlobalTransform,
) -> Option<Axis3d> {
    // EXTREMELY generous threshold - the entire ring area is clickable
    // Inner edge: radius * 0.3, Outer edge: radius * 1.7
    // This means clicking anywhere from 30% to 170% of the ring radius will work
    let inner_radius = radius * 0.3;
    let outer_radius = radius * 1.7;
    
    // Find the BEST axis match (closest to the ring radius)
    let mut best_axis: Option<Axis3d> = None;
    let mut best_score = f32::MAX;
    
    for axis in [Axis3d::X, Axis3d::Y, Axis3d::Z] {
        let axis_vec = axis.to_vec3();
        
        // Find intersection with plane perpendicular to axis
        if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, center, axis_vec) {
            let plane_hit = ray.origin + *ray.direction * t;
            let to_hit = plane_hit - center;
            let distance_to_center = to_hit.length();
            
            // Check if within the generous ring zone
            if distance_to_center >= inner_radius && distance_to_center <= outer_radius {
                // Score based on how close to the actual ring radius
                let ring_error = (distance_to_center - radius).abs();
                
                if ring_error < best_score {
                    best_score = ring_error;
                    best_axis = Some(axis);
                }
            }
        }
    }
    
    best_axis
}

/// Calculate rotation angle from ray intersection with rotation plane
fn calculate_rotation_angle(ray: &Ray3d, center: Vec3, axis: Axis3d) -> f32 {
    let axis_vec = axis.to_vec3();
    
    if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, center, axis_vec) {
        let plane_hit = ray.origin + *ray.direction * t;
        let to_hit = plane_hit - center;
        
        // Get reference vector perpendicular to axis
        let up = if axis_vec == Vec3::Y || axis_vec == -Vec3::Y {
            Vec3::X
        } else {
            Vec3::Y
        };
        
        let tangent1 = axis_vec.cross(up).normalize();
        
        // Calculate angle from reference
        let x = to_hit.dot(tangent1);
        let tangent2 = axis_vec.cross(tangent1).normalize();
        let y = to_hit.dot(tangent2);
        
        y.atan2(x)
    } else {
        0.0
    }
}

/// Snap angle to increments
fn snap_angle(angle: f32, increment: f32) -> f32 {
    if increment <= 0.0 {
        return angle;
    }
    
    (angle / increment).round() * increment
}
