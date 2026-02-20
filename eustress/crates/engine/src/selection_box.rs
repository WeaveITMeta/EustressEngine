// ============================================================================
// selection_box.rs — Roblox-style selection box visuals
// ============================================================================
//
// Table of Contents:
//   1. Imports & Constants
//   2. Components & Plugin
//   3. Gizmo Configuration
//   4. Hover Highlight Component
//   5. Selection Box Drawing (boxes, spheres, cylinders)
//   6. Corner Dot Drawing
//   7. Hover Highlight Drawing
//   8. BillboardGui Selection
//   9. Helper Geometry Functions
// ============================================================================

use bevy::prelude::*;
use bevy::gizmos::config::{GizmoConfigStore, DefaultGizmoConfigGroup};
use crate::classes::{BasePart, Part, PartType};
use crate::spawn::BillboardGuiMarker;

// ============================================================================
// 1. Constants
// ============================================================================

/// Primary selection outline color (Roblox-style bright cyan-blue)
const SELECTION_COLOR: Color = Color::srgba(0.35, 0.75, 1.0, 1.0);
/// Brighter highlight for the selection outline edges
const SELECTION_EDGE_COLOR: Color = Color::srgba(0.5, 0.9, 1.0, 1.0);
/// Corner dot color (white with slight blue tint)
const CORNER_DOT_COLOR: Color = Color::srgba(0.9, 0.97, 1.0, 1.0);
/// Hover highlight color (yellow-orange, semi-transparent)
const HOVER_COLOR: Color = Color::srgba(1.0, 0.85, 0.2, 0.75);
/// Corner dot radius in world units (scales with part size)
const CORNER_DOT_BASE_RADIUS: f32 = 0.06;
/// Minimum corner dot radius
const CORNER_DOT_MIN_RADIUS: f32 = 0.04;
/// Maximum corner dot radius
const CORNER_DOT_MAX_RADIUS: f32 = 0.18;

// ============================================================================
// 2. Components & Plugin
// ============================================================================

/// Component marking entities that should show selection boxes
#[derive(Component)]
pub struct SelectionBox;

/// Component marking entities that should show a hover highlight
#[derive(Component)]
pub struct HoverHighlight;

/// Plugin for managing Roblox-style selection box visuals
pub struct SelectionBoxPlugin;

impl Plugin for SelectionBoxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, configure_gizmos_on_top)
            .add_systems(Update, (
                draw_selection_boxes,
                draw_hover_highlights,
                draw_billboard_gui_selection,
            ));
    }
}

// ============================================================================
// 3. Gizmo Configuration
// ============================================================================

/// Configure gizmos to render on top at startup
fn configure_gizmos_on_top(mut config_store: ResMut<GizmoConfigStore>) {
    let (config, _) = config_store.config_mut::<DefaultGizmoConfigGroup>();
    config.depth_bias = -1.0; // Render on top of everything
    config.line.width = 2.0;  // Thicker lines for better visibility
}

// ============================================================================
// 4. Hover Highlight Drawing
// ============================================================================

/// Draw hover highlight outlines for hovered (but not selected) entities
fn draw_hover_highlights(
    mut gizmos: Gizmos,
    query: Query<(Entity, &GlobalTransform, Option<&BasePart>, Option<&Part>), (With<HoverHighlight>, Without<SelectionBox>, Without<BillboardGuiMarker>)>,
    children_query: Query<&Children>,
    all_transforms: Query<(&GlobalTransform, Option<&BasePart>), Without<BillboardGuiMarker>>,
    billboard_markers: Query<(), With<BillboardGuiMarker>>,
) {
    for (entity, transform, base_part, part) in &query {
        if let Ok(children) = children_query.get(entity) {
            if let Some((min, max)) = calculate_children_bounds(children, &all_transforms, &billboard_markers) {
                let center = (min + max) * 0.5;
                let size = max - min;
                draw_wireframe_box(&mut gizmos, center, Quat::IDENTITY, size, HOVER_COLOR, false);
                continue;
            }
        }

        let t = transform.compute_transform();
        let size = resolve_part_size(&t, base_part);
        if size.max_element() < 0.01 { continue; }

        let shape_type = part.map(|p| p.shape).unwrap_or(PartType::Block);
        match shape_type {
            PartType::Ball => {
                let radius = size.x / 2.0;
                draw_wireframe_sphere(&mut gizmos, t.translation, t.rotation, radius, HOVER_COLOR);
            }
            PartType::Cylinder => {
                let radius = size.x / 2.0;
                draw_wireframe_cylinder(&mut gizmos, t.translation, t.rotation, radius, size.y, HOVER_COLOR);
            }
            _ => {
                draw_wireframe_box(&mut gizmos, t.translation, t.rotation, size, HOVER_COLOR, false);
            }
        }
    }
}

// ============================================================================
// 5. Selection Box Drawing
// ============================================================================

/// Draw Roblox-style selection outlines using Bevy gizmos.
/// - Boxes for blocks/wedges, spheres for balls, cylinders for cylinders.
/// - For models with children, calculates combined bounding box.
/// - Draws corner dots at all 8 box corners.
/// - BillboardGui entities use a separate visualization.
fn draw_selection_boxes(
    mut gizmos: Gizmos,
    query: Query<(Entity, &GlobalTransform, Option<&BasePart>, Option<&Part>), (With<SelectionBox>, Without<BillboardGuiMarker>)>,
    children_query: Query<&Children>,
    all_transforms: Query<(&GlobalTransform, Option<&BasePart>), Without<BillboardGuiMarker>>,
    billboard_markers: Query<(), With<BillboardGuiMarker>>,
) {
    if query.is_empty() { return; }

    for (entity, transform, base_part, part) in &query {
        // Model with children: draw combined AABB
        if let Ok(children) = children_query.get(entity) {
            if let Some((min, max)) = calculate_children_bounds(children, &all_transforms, &billboard_markers) {
                let center = (min + max) * 0.5;
                let size = max - min;
                draw_wireframe_box(&mut gizmos, center, Quat::IDENTITY, size, SELECTION_COLOR, true);
                draw_corner_dots(&mut gizmos, center, Quat::IDENTITY, size);
                continue;
            }
        }

        let t = transform.compute_transform();
        let size = resolve_part_size(&t, base_part);
        if size.max_element() < 0.01 { continue; }

        let shape_type = part.map(|p| p.shape).unwrap_or(PartType::Block);
        match shape_type {
            PartType::Ball => {
                let radius = size.x / 2.0;
                draw_wireframe_sphere(&mut gizmos, t.translation, t.rotation, radius, SELECTION_COLOR);
                // Draw 6 cardinal dots on sphere surface
                draw_sphere_dots(&mut gizmos, t.translation, t.rotation, radius);
            }
            PartType::Cylinder => {
                let radius = size.x / 2.0;
                draw_wireframe_cylinder(&mut gizmos, t.translation, t.rotation, radius, size.y, SELECTION_COLOR);
            }
            _ => {
                draw_wireframe_box(&mut gizmos, t.translation, t.rotation, size, SELECTION_COLOR, true);
                draw_corner_dots(&mut gizmos, t.translation, t.rotation, size);
            }
        }
    }
}

// ============================================================================
// 6. Corner Dot Drawing
// ============================================================================

/// Draw small sphere dots at all 8 corners of the bounding box
fn draw_corner_dots(gizmos: &mut Gizmos, center: Vec3, rotation: Quat, size: Vec3) {
    let half = size * 0.5;
    // Scale dot radius with part size, clamped
    let dot_radius = (size.max_element() * 0.025)
        .max(CORNER_DOT_MIN_RADIUS)
        .min(CORNER_DOT_MAX_RADIUS);

    let local_corners = [
        Vec3::new(-half.x, -half.y, -half.z),
        Vec3::new( half.x, -half.y, -half.z),
        Vec3::new(-half.x,  half.y, -half.z),
        Vec3::new( half.x,  half.y, -half.z),
        Vec3::new(-half.x, -half.y,  half.z),
        Vec3::new( half.x, -half.y,  half.z),
        Vec3::new(-half.x,  half.y,  half.z),
        Vec3::new( half.x,  half.y,  half.z),
    ];

    for local in &local_corners {
        let world_pos = center + rotation * *local;
        // Draw a small cross (+) at each corner for crisp look at any distance
        let right = rotation * Vec3::X * dot_radius;
        let up    = rotation * Vec3::Y * dot_radius;
        let fwd   = rotation * Vec3::Z * dot_radius;
        gizmos.line(world_pos - right, world_pos + right, CORNER_DOT_COLOR);
        gizmos.line(world_pos - up,    world_pos + up,    CORNER_DOT_COLOR);
        gizmos.line(world_pos - fwd,   world_pos + fwd,   CORNER_DOT_COLOR);
    }
}

/// Draw dots at the 6 cardinal points on a sphere surface
fn draw_sphere_dots(gizmos: &mut Gizmos, center: Vec3, rotation: Quat, radius: f32) {
    let dot_radius = (radius * 0.05).max(CORNER_DOT_MIN_RADIUS).min(CORNER_DOT_MAX_RADIUS);
    let axes = [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z];
    for axis in &axes {
        let world_pos = center + rotation * (*axis * radius);
        let right = rotation * Vec3::X * dot_radius;
        let up    = rotation * Vec3::Y * dot_radius;
        let fwd   = rotation * Vec3::Z * dot_radius;
        gizmos.line(world_pos - right, world_pos + right, CORNER_DOT_COLOR);
        gizmos.line(world_pos - up,    world_pos + up,    CORNER_DOT_COLOR);
        gizmos.line(world_pos - fwd,   world_pos + fwd,   CORNER_DOT_COLOR);
    }
}

// ============================================================================
// 7. Helper: Resolve Part Size
// ============================================================================

/// Determine the visual size of a part from its transform and optional BasePart
fn resolve_part_size(t: &Transform, base_part: Option<&BasePart>) -> Vec3 {
    if (t.scale - Vec3::ONE).length() > 0.01 {
        t.scale
    } else if let Some(bp) = base_part {
        bp.size
    } else {
        t.scale
    }
}

// ============================================================================
// 8. BillboardGui Selection
// ============================================================================

/// Draw special selection visualization for BillboardGui entities
fn draw_billboard_gui_selection(
    mut gizmos: Gizmos,
    query: Query<(Entity, &GlobalTransform), (With<SelectionBox>, With<BillboardGuiMarker>)>,
    billboard_gui_query: Query<&eustress_common::classes::BillboardGui>,
) {
    let outline_color = Color::srgba(0.2, 0.5, 0.9, 0.8);

    for (entity, transform) in &query {
        let t = transform.compute_transform();

        let (width, height) = if let Ok(gui) = billboard_gui_query.get(entity) {
            (gui.size_offset[0], gui.size_offset[1])
        } else {
            (200.0, 50.0)
        };

        let world_width  = width  * 0.01;
        let world_height = height * 0.01;

        draw_billboard_rect(&mut gizmos, t.translation, t.rotation, world_width, world_height, SELECTION_COLOR);

        let outline_offset = 0.02;
        draw_billboard_rect(
            &mut gizmos,
            t.translation,
            t.rotation,
            world_width  + outline_offset * 2.0,
            world_height + outline_offset * 2.0,
            outline_color,
        );
    }
}

// ============================================================================
// 9. Helper Geometry Functions
// ============================================================================

/// Calculate combined bounding box for all children recursively.
/// Excludes BillboardGui entities.
fn calculate_children_bounds(
    children: &Children,
    all_transforms: &Query<(&GlobalTransform, Option<&crate::classes::BasePart>), Without<BillboardGuiMarker>>,
    billboard_markers: &Query<(), With<BillboardGuiMarker>>,
) -> Option<(Vec3, Vec3)> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut found_any = false;

    for child in children.iter() {
        if billboard_markers.get(child).is_ok() { continue; }

        if let Ok((child_transform, base_part)) = all_transforms.get(child) {
            let t = child_transform.compute_transform();
            let size = base_part.map(|bp| bp.size).unwrap_or(t.scale);
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

            for corner in &corners {
                let world = t.translation + t.rotation * *corner;
                min = min.min(world);
                max = max.max(world);
            }
            found_any = true;
        }
    }

    if found_any { Some((min, max)) } else { None }
}

/// Draw a wireframe box.
/// When `bright_edges` is true, draws a second pass with a brighter color for emphasis.
fn draw_wireframe_box(
    gizmos: &mut Gizmos,
    translation: Vec3,
    rotation: Quat,
    scale: Vec3,
    color: Color,
    bright_edges: bool,
) {
    let half = scale * 0.5;

    let local_corners = [
        Vec3::new(-half.x, -half.y, -half.z), // 0
        Vec3::new( half.x, -half.y, -half.z), // 1
        Vec3::new(-half.x,  half.y, -half.z), // 2
        Vec3::new( half.x,  half.y, -half.z), // 3
        Vec3::new(-half.x, -half.y,  half.z), // 4
        Vec3::new( half.x, -half.y,  half.z), // 5
        Vec3::new(-half.x,  half.y,  half.z), // 6
        Vec3::new( half.x,  half.y,  half.z), // 7
    ];

    let wc: Vec<Vec3> = local_corners.iter()
        .map(|&c| translation + rotation * c)
        .collect();

    // 12 edges
    let edges = [
        (0,1),(4,5),(0,4),(1,5), // bottom face
        (2,3),(6,7),(2,6),(3,7), // top face
        (0,2),(1,3),(4,6),(5,7), // verticals
    ];

    for (a, b) in &edges {
        gizmos.line(wc[*a], wc[*b], color);
    }

    // Second pass with brighter color for the top face edges (most visible)
    if bright_edges {
        let bright = SELECTION_EDGE_COLOR;
        gizmos.line(wc[2], wc[3], bright); // top-back
        gizmos.line(wc[6], wc[7], bright); // top-front
        gizmos.line(wc[2], wc[6], bright); // top-left
        gizmos.line(wc[3], wc[7], bright); // top-right
    }
}

/// Draw a wireframe sphere (3 orthogonal great circles)
fn draw_wireframe_sphere(
    gizmos: &mut Gizmos,
    translation: Vec3,
    rotation: Quat,
    radius: f32,
    color: Color,
) {
    let segments = 32;
    draw_circle_3d(gizmos, translation, rotation * Vec3::Z, radius, segments, color);
    draw_circle_3d(gizmos, translation, rotation * Vec3::Y, radius, segments, color);
    draw_circle_3d(gizmos, translation, rotation * Vec3::X, radius, segments, color);
}

/// Draw a wireframe cylinder (top/bottom circles + vertical lines)
fn draw_wireframe_cylinder(
    gizmos: &mut Gizmos,
    translation: Vec3,
    rotation: Quat,
    radius: f32,
    height: f32,
    color: Color,
) {
    let segments = 32;
    let half_h = height / 2.0;
    let up = rotation * Vec3::Y;
    let top    = translation + up * half_h;
    let bottom = translation - up * half_h;

    draw_circle_3d(gizmos, top,    up, radius, segments, color);
    draw_circle_3d(gizmos, bottom, up, radius, segments, color);

    for i in 0..8u32 {
        let angle = (i as f32 / 8.0) * std::f32::consts::TAU;
        let local = Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius);
        let world = rotation * local;
        gizmos.line(top + world, bottom + world, color);
    }
}

/// Draw a circle in 3D space defined by a center, normal, and radius
fn draw_circle_3d(
    gizmos: &mut Gizmos,
    center: Vec3,
    normal: Vec3,
    radius: f32,
    segments: u32,
    color: Color,
) {
    let up = if normal.dot(Vec3::Y).abs() > 0.99 { Vec3::X } else { Vec3::Y };
    let tangent   = normal.cross(up).normalize();
    let bitangent = tangent.cross(normal).normalize();

    let mut prev = center + tangent * radius;
    for i in 1..=segments {
        let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let point = center + (tangent * angle.cos() + bitangent * angle.sin()) * radius;
        gizmos.line(prev, point, color);
        prev = point;
    }
}

/// Draw a rectangle in 3D space (for BillboardGui selection)
fn draw_billboard_rect(
    gizmos: &mut Gizmos,
    center: Vec3,
    rotation: Quat,
    width: f32,
    height: f32,
    color: Color,
) {
    let hw = width  * 0.5;
    let hh = height * 0.5;

    let corners = [
        Vec3::new(-hw, -hh, 0.0),
        Vec3::new( hw, -hh, 0.0),
        Vec3::new( hw,  hh, 0.0),
        Vec3::new(-hw,  hh, 0.0),
    ];

    let wc: Vec<Vec3> = corners.iter()
        .map(|&c| center + rotation * c)
        .collect();

    gizmos.line(wc[0], wc[1], color);
    gizmos.line(wc[1], wc[2], color);
    gizmos.line(wc[2], wc[3], color);
    gizmos.line(wc[3], wc[0], color);
}
