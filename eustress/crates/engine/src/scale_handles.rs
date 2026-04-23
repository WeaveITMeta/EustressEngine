//! # Scale Tool Handles (Mesh-Based, Group-Aware)
//!
//! Structure parallels [`move_handles`](crate::move_handles) — one
//! singleton `ScaleHandleRoot` for the whole selection, positioned at
//! the group AABB center, with children laid out in the root's local
//! space. Every frame the root's transform is recomputed from the
//! current selection + active transform mode.
//!
//! ## Layout in root-local space
//!
//! Each axis arm: a thin cylinder shaft from origin to the face
//! distance, with a cube handle at the tip. 12 child entities (6
//! shafts + 6 cubes) plus 1 center cube for uniform scale = 13.
//!
//! ## Sizing
//!
//! Scale handles size themselves RELATIVE TO THE GROUP — the whole
//! arrangement scales with the combined bounding box. This matches
//! Unity / Maya convention where the scale cage visually "belongs to"
//! the object. A camera-distance MIN-SIZE floor is applied so
//! handles on tiny parts are still clickable.
//!
//! ## Transform mode
//!
//! - **World** → `Quat::IDENTITY`, axis cubes on world ±X/±Y/±Z
//! - **Local** → gizmo rotated by the active entity's rotation so
//!   axis cubes point along the part's local axes
//! - Multi-selection + Local → uses the last-iterated entity's
//!   rotation (same "active entity" convention as move_handles)

use bevy::prelude::*;
use eustress_common::adornments::{
    Adornment, BoxHandleAdornment, CylinderHandleAdornment,
};
use crate::adornment_renderer::AdornmentAxisColor;
use crate::selection_box::Selected;
use crate::scale_tool::{ScaleAxis, ScaleToolState};
use crate::classes::BasePart;
use crate::ui::{StudioState, Tool, TransformMode};
use crate::math_utils::calculate_rotated_aabb;

// ============================================================================
// Components
// ============================================================================

/// Singleton root for the Scale tool's handle set.
#[derive(Component)]
pub struct ScaleHandleRoot {
    /// Snapshot of the group AABB half-extent at last layout; used to
    /// detect significant size changes that require relayout.
    pub cached_extent: Vec3,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScaleHandle {
    pub axis: ScaleAxis,
}

/// Marker for cube-tip entities so hit-detection + color systems can
/// locate them specifically (the shaft cylinders are visual-only).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScaleCube;

// ============================================================================
// Constants
// ============================================================================

/// Handle extension past the face center, as a fraction of the group's
/// max half-extent. 0.35 puts the cube just outside the bounding box.
///
/// **Must match the hit-test in `scale_tool::handle_scale_interaction`** —
/// these constants live here but are consumed both by the visual layout
/// and by the click-target math to keep them in lockstep.
pub const HANDLE_EXT_FRAC: f32 = 0.35;
/// Cube size as a fraction of max half-extent.
pub const CUBE_SIZE_FRAC: f32 = 0.14;
/// Center-cube (uniform scale) larger so it doesn't fight the face cubes.
pub const CENTER_CUBE_FRAC: f32 = 0.20;
/// Shaft radius as a fraction of max half-extent — arrow-like thinness.
pub const SHAFT_RADIUS_FRAC: f32 = 0.020;
/// Camera-distance MIN size: even for a 0.01-unit part, handles stay
/// at least this fraction of the screen height.
pub const MIN_SCREEN_FRACTION: f32 = 0.08;

// ============================================================================
// Plugin
// ============================================================================

pub struct ScaleHandlesPlugin;

impl Plugin for ScaleHandlesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (
                sync_scale_handle_root,
                update_handle_colors,
            ),
        );
    }
}

// ============================================================================
// Core system — spawn/despawn and per-frame update
// ============================================================================

/// One-pass owner of the `ScaleHandleRoot` singleton:
/// 1. If tool ≠ Scale OR no selection → despawn root, exit.
/// 2. If root missing → spawn.
/// 3. Compute group AABB center + rotation + handle sizing.
/// 4. Write root Transform.
/// 5. Reposition children if the group extent changed (different part
///    size requires different cube positions).
fn sync_scale_handle_root(
    mut commands: Commands,
    studio_state: Option<Res<StudioState>>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
    selected: Query<
        (Entity, &GlobalTransform, Option<&BasePart>),
        With<Selected>,
    >,
    mut root_query: Query<(&mut ScaleHandleRoot, &mut Transform)>,
    existing_root: Query<Entity, With<ScaleHandleRoot>>,
    mut child_handles: Query<
        (Entity, &ScaleHandle, &mut Transform, &mut BoxHandleAdornment),
        (With<ScaleCube>, Without<ScaleHandleRoot>),
    >,
    mut child_shafts: Query<
        (&ScaleHandle, &mut Transform, &mut CylinderHandleAdornment),
        (Without<ScaleCube>, Without<ScaleHandleRoot>, Without<BoxHandleAdornment>),
    >,
) {
    let Some(studio_state) = studio_state else { return };

    let tool_active = matches!(studio_state.current_tool, Tool::Scale);
    let has_selection = !selected.is_empty();

    // Despawn when not applicable.
    if !tool_active || !has_selection {
        for e in existing_root.iter() {
            commands.entity(e).despawn();
        }
        return;
    }

    // Spawn singleton if missing. Child layout happens next frame once
    // the root component has been committed — acceptable 1-frame lag.
    if existing_root.iter().next().is_none() {
        spawn_scale_handle_root(&mut commands);
        return;
    }

    // Group bounds over ALL selected entities.
    let Some(group) = compute_group_frame(
        selected.iter().map(|(e, gt, bp)| (e, gt, bp)),
    ) else {
        return;
    };

    // Camera-distance min-size floor so tiny parts still get usable handles.
    let Some((_, cam_gt, projection)) = cameras.iter().find(|(c, _, _)| c.order == 0) else {
        return;
    };
    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => std::f32::consts::FRAC_PI_4,
    };
    let cam_dist = (group.center - cam_gt.translation()).length().max(0.1);
    let min_screen_size = cam_dist * (fov * 0.5).tan() * MIN_SCREEN_FRACTION;

    // Effective half-extent used for sizing: max of size-based and screen-min.
    let size_extent = group.extent.max_element();
    let effective_extent = size_extent.max(min_screen_size);

    // Root rotation per transform mode.
    let rotation = match studio_state.transform_mode {
        TransformMode::World => Quat::IDENTITY,
        TransformMode::Local => group.active_rotation,
    };

    // Write root transform.
    let Ok((mut root, mut root_transform)) = root_query.single_mut() else { return };
    root_transform.translation = group.center;
    root_transform.rotation = rotation;
    root_transform.scale = Vec3::ONE;

    // If extent changed materially, relayout children. Use a squared
    // tolerance to avoid re-layouts from floating-point jitter.
    let cached = root.cached_extent;
    if (cached.length_squared() - Vec3::splat(effective_extent).length_squared()).abs() > 1e-6
        || cached == Vec3::ZERO
    {
        root.cached_extent = Vec3::splat(effective_extent);
        layout_scale_children(
            effective_extent,
            &mut child_handles,
            &mut child_shafts,
        );
    }
}

// ----------------------------------------------------------------------------
// Group frame helper — same pattern as move_handles
// ----------------------------------------------------------------------------

struct GroupFrame {
    center: Vec3,
    extent: Vec3,              // half-size of the AABB
    active_rotation: Quat,
}

fn compute_group_frame<'a>(
    iter: impl Iterator<Item = (Entity, &'a GlobalTransform, Option<&'a BasePart>)>,
) -> Option<GroupFrame> {
    let mut bounds_min = Vec3::splat(f32::MAX);
    let mut bounds_max = Vec3::splat(f32::MIN);
    let mut count = 0;
    let mut last_rotation = Quat::IDENTITY;

    for (_e, gt, base_part) in iter {
        let t = gt.compute_transform();
        let size = base_part.map(|bp| bp.size).unwrap_or(t.scale);
        let (mn, mx) = calculate_rotated_aabb(t.translation, size * 0.5, t.rotation);
        bounds_min = bounds_min.min(mn);
        bounds_max = bounds_max.max(mx);
        last_rotation = t.rotation;
        count += 1;
    }
    if count == 0 { return None; }

    let center = (bounds_min + bounds_max) * 0.5;
    let extent = (bounds_max - bounds_min) * 0.5;
    Some(GroupFrame { center, extent, active_rotation: last_rotation })
}

// ============================================================================
// Spawn — once per session
// ============================================================================

fn spawn_scale_handle_root(commands: &mut Commands) {
    let root = commands.spawn((
        ScaleHandleRoot { cached_extent: Vec3::ZERO },
        Adornment { meta: true },
        Transform::IDENTITY,
        Visibility::default(),
        Name::new("ScaleHandleRoot"),
    )).id();

    // 6 face arms with placeholder transforms; real positions set by
    // layout_scale_children on the next frame.
    let arms = [
        (ScaleAxis::XPos, Vec3::X,     AdornmentAxisColor::X),
        (ScaleAxis::XNeg, Vec3::NEG_X, AdornmentAxisColor::X),
        (ScaleAxis::YPos, Vec3::Y,     AdornmentAxisColor::Y),
        (ScaleAxis::YNeg, Vec3::NEG_Y, AdornmentAxisColor::Y),
        (ScaleAxis::ZPos, Vec3::Z,     AdornmentAxisColor::Z),
        (ScaleAxis::ZNeg, Vec3::NEG_Z, AdornmentAxisColor::Z),
    ];

    for (axis, dir, color) in arms {
        spawn_scale_arm(commands, root, axis, dir, color);
    }

    // Center uniform cube.
    commands.spawn((
        BoxHandleAdornment {
            size: Vec3::splat(1.0),   // replaced in layout_scale_children
            shading: Default::default(),
        },
        AdornmentAxisColor::Center,
        ScaleHandle { axis: ScaleAxis::Uniform },
        ScaleCube,
        Adornment { meta: true },
        Transform::IDENTITY,
        Visibility::default(),
        Name::new("ScaleHandle.Uniform"),
        ChildOf(root),
    ));
}

fn spawn_scale_arm(
    commands: &mut Commands,
    root: Entity,
    axis: ScaleAxis,
    dir: Vec3,
    color: AdornmentAxisColor,
) {
    let rot = rotation_aligning_y_to(dir);

    // Shaft — placeholder size, real values set in layout_scale_children.
    commands.spawn((
        CylinderHandleAdornment {
            height: 1.0,
            radius: 0.02,
            inner_radius: 0.0,
            angle: 360.0,
            shading: Default::default(),
        },
        color,
        ScaleHandle { axis },
        Adornment { meta: true },
        Transform { translation: Vec3::ZERO, rotation: rot, scale: Vec3::ONE },
        Visibility::default(),
        Name::new(arm_name("Shaft", axis)),
        ChildOf(root),
    ));

    // Cube — placeholder size.
    commands.spawn((
        BoxHandleAdornment {
            size: Vec3::splat(1.0),
            shading: Default::default(),
        },
        color,
        ScaleHandle { axis },
        ScaleCube,
        Adornment { meta: true },
        Transform { translation: Vec3::ZERO, rotation: rot, scale: Vec3::ONE },
        Visibility::default(),
        Name::new(arm_name("Cube", axis)),
        ChildOf(root),
    ));
}

// ============================================================================
// Layout — reposition children based on current group extent
// ============================================================================

fn layout_scale_children(
    extent: f32,
    cubes: &mut Query<
        (Entity, &ScaleHandle, &mut Transform, &mut BoxHandleAdornment),
        (With<ScaleCube>, Without<ScaleHandleRoot>),
    >,
    shafts: &mut Query<
        (&ScaleHandle, &mut Transform, &mut CylinderHandleAdornment),
        (Without<ScaleCube>, Without<ScaleHandleRoot>, Without<BoxHandleAdornment>),
    >,
) {
    let extent = extent.max(0.001);
    let handle_ext = extent * HANDLE_EXT_FRAC;
    let cube_size = extent * CUBE_SIZE_FRAC;
    let center_size = extent * CENTER_CUBE_FRAC;
    let shaft_radius = extent * SHAFT_RADIUS_FRAC;

    // Cubes: per-axis at `extent + handle_ext`. Center at origin.
    for (_e, handle, mut t, mut bx) in cubes.iter_mut() {
        let (dir, is_center) = match handle.axis {
            ScaleAxis::XPos => (Vec3::X,     false),
            ScaleAxis::XNeg => (Vec3::NEG_X, false),
            ScaleAxis::YPos => (Vec3::Y,     false),
            ScaleAxis::YNeg => (Vec3::NEG_Y, false),
            ScaleAxis::ZPos => (Vec3::Z,     false),
            ScaleAxis::ZNeg => (Vec3::NEG_Z, false),
            ScaleAxis::Uniform => (Vec3::ZERO, true),
        };
        if is_center {
            t.translation = Vec3::ZERO;
            bx.size = Vec3::splat(center_size);
        } else {
            t.translation = dir * (extent + handle_ext);
            bx.size = Vec3::splat(cube_size);
        }
    }

    // Shafts: midpoint between face and cube, height = handle_ext.
    for (handle, mut t, mut cyl) in shafts.iter_mut() {
        let dir = match handle.axis {
            ScaleAxis::XPos => Vec3::X,
            ScaleAxis::XNeg => Vec3::NEG_X,
            ScaleAxis::YPos => Vec3::Y,
            ScaleAxis::YNeg => Vec3::NEG_Y,
            ScaleAxis::ZPos => Vec3::Z,
            ScaleAxis::ZNeg => Vec3::NEG_Z,
            ScaleAxis::Uniform => continue,   // no shaft for center
        };
        t.translation = dir * (extent + handle_ext * 0.5);
        cyl.height = handle_ext;
        cyl.radius = shaft_radius;
    }
}

fn rotation_aligning_y_to(dir: Vec3) -> Quat {
    let d = dir.normalize_or_zero();
    if d.length_squared() < 0.0001 { return Quat::IDENTITY; }
    if d.dot(Vec3::Y) < -0.9999 {
        return Quat::from_axis_angle(Vec3::X, std::f32::consts::PI);
    }
    Quat::from_rotation_arc(Vec3::Y, d)
}

fn arm_name(kind: &str, axis: ScaleAxis) -> String {
    format!("ScaleHandle.{:?}.{}", axis, kind)
}

// ============================================================================
// Hover / drag color swap
// ============================================================================

fn update_handle_colors(
    state: Option<Res<ScaleToolState>>,
    mut handles: Query<(&ScaleHandle, &mut AdornmentAxisColor)>,
) {
    let Some(state) = state else { return };

    for (h, mut color) in handles.iter_mut() {
        let desired = if state.dragged_axis == Some(h.axis) {
            AdornmentAxisColor::Drag
        } else {
            match h.axis {
                ScaleAxis::XPos | ScaleAxis::XNeg => AdornmentAxisColor::X,
                ScaleAxis::YPos | ScaleAxis::YNeg => AdornmentAxisColor::Y,
                ScaleAxis::ZPos | ScaleAxis::ZNeg => AdornmentAxisColor::Z,
                ScaleAxis::Uniform => AdornmentAxisColor::Center,
            }
        };
        if *color != desired {
            *color = desired;
        }
    }
}
