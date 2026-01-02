use bevy::prelude::*;
use bevy::gizmos::config::{GizmoConfigStore, DefaultGizmoConfigGroup};
use crate::classes::{BasePart, Part, PartType};
use crate::spawn::BillboardGuiMarker;

/// Component marking entities that should show selection boxes
#[derive(Component)]
pub struct SelectionBox;

/// Plugin for managing Roblox-style selection box visuals
pub struct SelectionBoxPlugin;

impl Plugin for SelectionBoxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, configure_gizmos_on_top)
            .add_systems(Update, (draw_selection_boxes, draw_billboard_gui_selection));
    }
}

/// Configure gizmos to render on top at startup
fn configure_gizmos_on_top(mut config_store: ResMut<GizmoConfigStore>) {
    let (config, _) = config_store.config_mut::<DefaultGizmoConfigGroup>();
    config.depth_bias = -1.0; // Render on top of everything
}

/// Draw Roblox-style selection outlines using Bevy gizmos
/// Draws shape-appropriate outlines: boxes for blocks, spheres for balls, cylinders for cylinders
/// For models with children, calculates combined bounding box
/// Note: BillboardGui entities are excluded - they use a different selection visualization
fn draw_selection_boxes(
    mut gizmos: Gizmos,
    query: Query<(Entity, &GlobalTransform, Option<&BasePart>, Option<&Part>), (With<SelectionBox>, Without<BillboardGuiMarker>)>,
    children_query: Query<&Children>,
    all_transforms: Query<(&GlobalTransform, Option<&BasePart>), Without<BillboardGuiMarker>>,
    billboard_markers: Query<(), With<BillboardGuiMarker>>,
) {
    // Don't draw if no parts are selected
    if query.is_empty() {
        return;
    }
    
    // Roblox-style light blue selection color
    let selection_color = Color::srgba(0.3, 0.7, 1.0, 1.0); // Light cyan/blue
    
    for (entity, transform, base_part, part) in &query {
        // Check if this entity has children (is a model)
        if let Ok(children) = children_query.get(entity) {
            // Calculate combined bounding box for all children (excluding BillboardGui)
            if let Some((min, max)) = calculate_children_bounds(children, &all_transforms, &billboard_markers) {
                let center = (min + max) * 0.5;
                let size = max - min;
                
                // Draw bounding box at combined bounds
                draw_wireframe_box(&mut gizmos, center, Quat::IDENTITY, size, selection_color);
                continue;
            }
        }
        
        // No children or calculation failed - draw shape-appropriate outline
        let t = transform.compute_transform();
        
        // Calculate the ACTUAL visual size of the part
        // The visual size depends on how the mesh was created:
        // - If mesh is at actual size: transform.scale should be Vec3::ONE, use BasePart.size
        // - If mesh is at unit size: transform.scale IS the size
        // We check transform.scale first since it represents the actual rendered size
        let size = if (t.scale - Vec3::ONE).length() > 0.01 {
            // transform.scale is not identity - use it as the visual size
            t.scale
        } else if let Some(bp) = base_part {
            // transform.scale is ~1, use BasePart.size
            bp.size
        } else {
            // Fallback to scale
            t.scale
        };
        
        // Skip drawing if size is too small (likely artifact/uninitialized)
        if size.max_element() < 0.01 {
            continue;
        }
        
        // Determine shape type and draw appropriate outline
        let shape_type = part.map(|p| p.shape).unwrap_or(PartType::Block);
        
        match shape_type {
            PartType::Ball => {
                // Draw wireframe sphere for balls
                let radius = size.x / 2.0; // Ball uses size.x as diameter
                draw_wireframe_sphere(&mut gizmos, t.translation, t.rotation, radius, selection_color);
            }
            PartType::Cylinder => {
                // Draw wireframe cylinder
                let radius = size.x / 2.0; // Cylinder uses size.x as diameter
                let height = size.y;
                draw_wireframe_cylinder(&mut gizmos, t.translation, t.rotation, radius, height, selection_color);
            }
            _ => {
                // Draw wireframe box for blocks, wedges, etc.
                draw_wireframe_box(&mut gizmos, t.translation, t.rotation, size, selection_color);
            }
        }
    }
}

/// Calculate combined bounding box for all children recursively
/// Excludes BillboardGui entities from the calculation
fn calculate_children_bounds(
    children: &Children,
    all_transforms: &Query<(&GlobalTransform, Option<&crate::classes::BasePart>), Without<BillboardGuiMarker>>,
    billboard_markers: &Query<(), With<BillboardGuiMarker>>,
) -> Option<(Vec3, Vec3)> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut found_any = false;
    
    for child in children.iter() {
        // Skip BillboardGui entities - they shouldn't affect parent selection box
        if billboard_markers.get(child).is_ok() {
            continue;
        }
        
        if let Ok((child_transform, base_part)) = all_transforms.get(child) {
            let t = child_transform.compute_transform();
            // Use BasePart.size if available, otherwise fall back to transform scale
            let size = if let Some(bp) = base_part {
                bp.size
            } else {
                t.scale
            };
            let half_size = size * 0.5;
            
            // Calculate 8 corners of this child's bounding box
            let corners = [
                t.translation + t.rotation.mul_vec3(Vec3::new(-half_size.x, -half_size.y, -half_size.z)),
                t.translation + t.rotation.mul_vec3(Vec3::new( half_size.x, -half_size.y, -half_size.z)),
                t.translation + t.rotation.mul_vec3(Vec3::new(-half_size.x,  half_size.y, -half_size.z)),
                t.translation + t.rotation.mul_vec3(Vec3::new( half_size.x,  half_size.y, -half_size.z)),
                t.translation + t.rotation.mul_vec3(Vec3::new(-half_size.x, -half_size.y,  half_size.z)),
                t.translation + t.rotation.mul_vec3(Vec3::new( half_size.x, -half_size.y,  half_size.z)),
                t.translation + t.rotation.mul_vec3(Vec3::new(-half_size.x,  half_size.y,  half_size.z)),
                t.translation + t.rotation.mul_vec3(Vec3::new( half_size.x,  half_size.y,  half_size.z)),
            ];
            
            // Update min/max bounds
            for corner in &corners {
                min = min.min(*corner);
                max = max.max(*corner);
            }
            found_any = true;
        }
    }
    
    if found_any {
        Some((min, max))
    } else {
        None
    }
}

/// Draw a wireframe box at the given transform
/// Creates 12 edges of a cube for a clean selection outline
fn draw_wireframe_box(
    gizmos: &mut Gizmos,
    translation: Vec3,
    rotation: Quat,
    scale: Vec3,
    color: Color,
) {
    // Half extents for the box
    let half_size = scale * 0.5;
    
    // 8 corners of the box in local space
    let corners = [
        Vec3::new(-half_size.x, -half_size.y, -half_size.z), // 0: bottom-back-left
        Vec3::new( half_size.x, -half_size.y, -half_size.z), // 1: bottom-back-right
        Vec3::new(-half_size.x,  half_size.y, -half_size.z), // 2: top-back-left
        Vec3::new( half_size.x,  half_size.y, -half_size.z), // 3: top-back-right
        Vec3::new(-half_size.x, -half_size.y,  half_size.z), // 4: bottom-front-left
        Vec3::new( half_size.x, -half_size.y,  half_size.z), // 5: bottom-front-right
        Vec3::new(-half_size.x,  half_size.y,  half_size.z), // 6: top-front-left
        Vec3::new( half_size.x,  half_size.y,  half_size.z), // 7: top-front-right
    ];
    
    // Transform corners to world space
    let world_corners: Vec<Vec3> = corners
        .iter()
        .map(|&corner| translation + rotation.mul_vec3(corner))
        .collect();
    
    // Draw 12 edges of the cube
    // Bottom face (4 edges)
    gizmos.line(world_corners[0], world_corners[1], color); // back
    gizmos.line(world_corners[4], world_corners[5], color); // front
    gizmos.line(world_corners[0], world_corners[4], color); // left
    gizmos.line(world_corners[1], world_corners[5], color); // right
    
    // Top face (4 edges)
    gizmos.line(world_corners[2], world_corners[3], color); // back
    gizmos.line(world_corners[6], world_corners[7], color); // front
    gizmos.line(world_corners[2], world_corners[6], color); // left
    gizmos.line(world_corners[3], world_corners[7], color); // right
    
    // Vertical edges (4 edges)
    gizmos.line(world_corners[0], world_corners[2], color); // back-left
    gizmos.line(world_corners[1], world_corners[3], color); // back-right
    gizmos.line(world_corners[4], world_corners[6], color); // front-left
    gizmos.line(world_corners[5], world_corners[7], color); // front-right
}

/// Draw a wireframe sphere at the given position
/// Creates 3 orthogonal circles for a clean spherical selection outline
fn draw_wireframe_sphere(
    gizmos: &mut Gizmos,
    translation: Vec3,
    rotation: Quat,
    radius: f32,
    color: Color,
) {
    let segments = 32;
    
    // Draw 3 orthogonal circles (XY, XZ, YZ planes)
    // XY plane (horizontal when looking down Z)
    draw_circle_3d(gizmos, translation, rotation * Vec3::Z, radius, segments, color);
    // XZ plane (vertical when looking down Y)  
    draw_circle_3d(gizmos, translation, rotation * Vec3::Y, radius, segments, color);
    // YZ plane (vertical when looking down X)
    draw_circle_3d(gizmos, translation, rotation * Vec3::X, radius, segments, color);
}

/// Draw a wireframe cylinder at the given position
/// Creates top/bottom circles and vertical lines
fn draw_wireframe_cylinder(
    gizmos: &mut Gizmos,
    translation: Vec3,
    rotation: Quat,
    radius: f32,
    height: f32,
    color: Color,
) {
    let segments = 32;
    let half_height = height / 2.0;
    
    // Top and bottom circle centers
    let up = rotation * Vec3::Y;
    let top_center = translation + up * half_height;
    let bottom_center = translation - up * half_height;
    
    // Draw top and bottom circles
    draw_circle_3d(gizmos, top_center, up, radius, segments, color);
    draw_circle_3d(gizmos, bottom_center, up, radius, segments, color);
    
    // Draw vertical lines connecting top and bottom
    let vertical_lines = 8;
    for i in 0..vertical_lines {
        let angle = (i as f32 / vertical_lines as f32) * std::f32::consts::TAU;
        let local_offset = Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius);
        let world_offset = rotation * local_offset;
        
        let top_point = top_center + world_offset;
        let bottom_point = bottom_center + world_offset;
        gizmos.line(top_point, bottom_point, color);
    }
}

/// Helper to draw a circle in 3D space
fn draw_circle_3d(
    gizmos: &mut Gizmos,
    center: Vec3,
    normal: Vec3,
    radius: f32,
    segments: u32,
    color: Color,
) {
    // Create orthonormal basis from normal
    let up = if normal.dot(Vec3::Y).abs() > 0.99 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let tangent = normal.cross(up).normalize();
    let bitangent = tangent.cross(normal).normalize();
    
    // Draw circle as line segments
    let mut prev_point = center + tangent * radius;
    for i in 1..=segments {
        let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let point = center + (tangent * angle.cos() + bitangent * angle.sin()) * radius;
        gizmos.line(prev_point, point, color);
        prev_point = point;
    }
}

/// Draw special selection visualization for BillboardGui entities
/// Instead of a 3D selection box, draws a blue outline around the billboard in 3D space
/// with a slightly expanded outline (+2 pixels equivalent) for visual clarity
fn draw_billboard_gui_selection(
    mut gizmos: Gizmos,
    query: Query<(Entity, &GlobalTransform), (With<SelectionBox>, With<BillboardGuiMarker>)>,
    billboard_gui_query: Query<&eustress_common::classes::BillboardGui>,
) {
    // Selection blue color (Roblox-style)
    let selection_color = Color::srgba(0.3, 0.7, 1.0, 1.0);
    // Slightly darker/more saturated for the outer outline
    let outline_color = Color::srgba(0.2, 0.5, 0.9, 0.8);
    
    for (entity, transform) in &query {
        let t = transform.compute_transform();
        
        // Get BillboardGui size if available, otherwise use default
        let (width, height) = if let Ok(gui) = billboard_gui_query.get(entity) {
            (gui.size_offset[0], gui.size_offset[1])
        } else {
            (200.0, 50.0) // Default billboard size
        };
        
        // Convert pixel size to world units (approximate: 1 pixel = 0.01 studs for billboards)
        let world_width = width * 0.01;
        let world_height = height * 0.01;
        
        // Draw inner rectangle (the actual billboard bounds)
        draw_billboard_rect(&mut gizmos, t.translation, t.rotation, world_width, world_height, selection_color);
        
        // Draw outer rectangle (+2 pixels = +0.02 studs per side for the outline effect)
        let outline_offset = 0.02;
        draw_billboard_rect(
            &mut gizmos, 
            t.translation, 
            t.rotation, 
            world_width + outline_offset * 2.0, 
            world_height + outline_offset * 2.0, 
            outline_color
        );
    }
}

/// Draw a rectangle in 3D space facing the camera (billboard-style)
fn draw_billboard_rect(
    gizmos: &mut Gizmos,
    center: Vec3,
    rotation: Quat,
    width: f32,
    height: f32,
    color: Color,
) {
    let half_w = width * 0.5;
    let half_h = height * 0.5;
    
    // Billboard corners in local space (facing +Z by default)
    let corners = [
        Vec3::new(-half_w, -half_h, 0.0), // bottom-left
        Vec3::new( half_w, -half_h, 0.0), // bottom-right
        Vec3::new( half_w,  half_h, 0.0), // top-right
        Vec3::new(-half_w,  half_h, 0.0), // top-left
    ];
    
    // Transform to world space
    let world_corners: Vec<Vec3> = corners
        .iter()
        .map(|&corner| center + rotation.mul_vec3(corner))
        .collect();
    
    // Draw 4 edges of the rectangle
    gizmos.line(world_corners[0], world_corners[1], color);
    gizmos.line(world_corners[1], world_corners[2], color);
    gizmos.line(world_corners[2], world_corners[3], color);
    gizmos.line(world_corners[3], world_corners[0], color);
}
