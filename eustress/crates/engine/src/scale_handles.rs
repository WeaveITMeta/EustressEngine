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
//
// All sizing is camera-distance-based so handles stay at constant screen
// size regardless of object size. POSITIONING is AABB-anchored — cubes
// sit at `face_center + screen_offset` so they always hug the part.
//
// **Must match the hit-test in `scale_tool::handle_scale_interaction`** —
// these constants live here but are consumed both by the visual layout
// and by the click-target math to keep them in lockstep. Same screen-
// scale formula as `move_handles.rs` (SCREEN_FRACTION pattern) so Move
// and Scale tools feel identically sized at any zoom level.

/// Base screen fraction — handle-set "scale" is `dist * tan(fov/2) * SCREEN_FRACTION`.
/// 0.16 matches `move_handles::SCREEN_FRACTION` so the two tools read alike.
pub const SCREEN_FRACTION: f32 = 0.16;
/// Cube offset past the face center, in screen-scale units. The cube
/// sits just outside the part regardless of object size.
pub const SCREEN_HANDLE_EXT: f32 = 0.35;
/// Cube edge length, in screen-scale units. ~big enough to click without
/// crowding the surface.
pub const SCREEN_CUBE_SIZE: f32 = 0.18;
/// Center (uniform-scale) cube edge length — slightly larger than face
/// cubes so it doesn't fight them at the origin.
pub const SCREEN_CENTER_SIZE: f32 = 0.25;
/// Shaft cylinder radius, in screen-scale units. Arrow-like thinness.
pub const SCREEN_SHAFT_RADIUS: f32 = 0.025;

// ============================================================================
// Plugin
// ============================================================================

pub struct ScaleHandlesPlugin;

impl Plugin for ScaleHandlesPlugin {
    fn build(&self, app: &mut App) {
        // Run BEFORE Bevy's transform propagation — same rationale as
        // `MoveHandlesPlugin`: otherwise the handle root trails the
        // part by one frame every time the scale tool writes a new
        // Transform.
        app.add_systems(
            PostUpdate,
            (
                sync_scale_handle_root,
                update_handle_colors,
            )
                .before(bevy::transform::TransformSystems::Propagate),
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
    // Camera still uses `GlobalTransform` — see move_handles for why.
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
    // Selected parts: LOCAL `Transform` so this frame's tool write is
    // visible without waiting on propagate. Identity-parent assumption.
    selected: Query<
        (Entity, &Transform, Option<&BasePart>),
        With<Selected>,
    >,
    // `Without<Selected>` on every mutable-Transform query disjoints
    // them from the `selected` read-only query above, which now reads
    // `&Transform` directly (the switch from `&GlobalTransform` that
    // fixed the 1-frame gizmo-lag bug). Without these filters Bevy
    // panics B0001 on startup because both a shared and exclusive
    // Transform reference can overlap on the same entity.
    mut root_query: Query<(&mut ScaleHandleRoot, &mut Transform), Without<Selected>>,
    existing_root: Query<Entity, With<ScaleHandleRoot>>,
    mut child_handles: Query<
        (Entity, &ScaleHandle, &mut Transform, &mut BoxHandleAdornment),
        (With<ScaleCube>, Without<ScaleHandleRoot>, Without<Selected>),
    >,
    mut child_shafts: Query<
        (&ScaleHandle, &mut Transform, &mut CylinderHandleAdornment),
        (Without<ScaleCube>, Without<ScaleHandleRoot>, Without<BoxHandleAdornment>, Without<Selected>),
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
        selected.iter().map(|(e, t, bp)| (e, t, bp)),
    ) else {
        return;
    };

    // Camera-distance scale — handles stay at constant screen size
    // regardless of object size. Same formula as move_handles.
    let Some((_, cam_gt, projection)) = cameras.iter().find(|(c, _, _)| c.order == 0) else {
        return;
    };
    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => std::f32::consts::FRAC_PI_4,
    };
    let cam_dist = (group.center - cam_gt.translation()).length().max(0.1);
    let screen_scale = cam_dist * (fov * 0.5).tan() * SCREEN_FRACTION;

    // Root rotation per transform mode.
    let rotation = match studio_state.transform_mode {
        TransformMode::World => Quat::IDENTITY,
        TransformMode::Local => group.active_rotation,
    };

    // Write root transform. Root.scale stays Vec3::ONE — children carry
    // their own world-space sizes (we don't multiply through root.scale
    // because cube positions need per-axis face anchoring, not uniform
    // unit-scale + multiply).
    let Ok((mut root, mut root_transform)) = root_query.single_mut() else { return };
    root_transform.translation = group.center;
    root_transform.rotation = rotation;
    root_transform.scale = Vec3::ONE;

    // Relayout every frame — sizes depend on camera distance (screen_scale)
    // AND positions depend on per-axis extent. 13 children, no allocation;
    // cheaper than tracking deltas across two inputs that both change as
    // soon as the user moves the camera or resizes the part.
    root.cached_extent = group.extent;
    layout_scale_children(
        group.extent,
        screen_scale,
        &mut child_handles,
        &mut child_shafts,
    );
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
    iter: impl Iterator<Item = (Entity, &'a Transform, Option<&'a BasePart>)>,
) -> Option<GroupFrame> {
    let mut bounds_min = Vec3::splat(f32::MAX);
    let mut bounds_max = Vec3::splat(f32::MIN);
    let mut count = 0;
    let mut last_rotation = Quat::IDENTITY;

    for (_e, t, base_part) in iter {
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

/// Per-axis face extent (so a tall thin part puts Y handles further out
/// than X handles), with constant screen-size cubes regardless of part
/// dimensions.
fn layout_scale_children(
    extent: Vec3,
    screen_scale: f32,
    // Filter tuples must match the caller's query exactly — the
    // `Without<Selected>` disjoints them from the `selected` read-only
    // `&Transform` query in `sync_scale_handle_root`, which satisfies
    // Bevy's B0001 overlap check.
    cubes: &mut Query<
        (Entity, &ScaleHandle, &mut Transform, &mut BoxHandleAdornment),
        (With<ScaleCube>, Without<ScaleHandleRoot>, Without<Selected>),
    >,
    shafts: &mut Query<
        (&ScaleHandle, &mut Transform, &mut CylinderHandleAdornment),
        (Without<ScaleCube>, Without<ScaleHandleRoot>, Without<BoxHandleAdornment>, Without<Selected>),
    >,
) {
    let extent = extent.max(Vec3::splat(0.001));
    let screen_scale = screen_scale.max(0.001);

    // Constant world-space dims derived from camera distance.
    let handle_ext = screen_scale * SCREEN_HANDLE_EXT;
    let cube_size = screen_scale * SCREEN_CUBE_SIZE;
    let center_size = screen_scale * SCREEN_CENTER_SIZE;
    let shaft_radius = screen_scale * SCREEN_SHAFT_RADIUS;

    // Cubes: per-axis at `face_extent + handle_ext`. Center at origin.
    for (_e, handle, mut t, mut bx) in cubes.iter_mut() {
        let (dir, face, is_center) = match handle.axis {
            ScaleAxis::XPos => (Vec3::X,     extent.x, false),
            ScaleAxis::XNeg => (Vec3::NEG_X, extent.x, false),
            ScaleAxis::YPos => (Vec3::Y,     extent.y, false),
            ScaleAxis::YNeg => (Vec3::NEG_Y, extent.y, false),
            ScaleAxis::ZPos => (Vec3::Z,     extent.z, false),
            ScaleAxis::ZNeg => (Vec3::NEG_Z, extent.z, false),
            ScaleAxis::Uniform => (Vec3::ZERO, 0.0, true),
        };
        if is_center {
            t.translation = Vec3::ZERO;
            bx.size = Vec3::splat(center_size);
        } else {
            t.translation = dir * (face + handle_ext);
            bx.size = Vec3::splat(cube_size);
        }
    }

    // Shafts: from face to just under the cube, constant radius.
    for (handle, mut t, mut cyl) in shafts.iter_mut() {
        let (dir, face) = match handle.axis {
            ScaleAxis::XPos => (Vec3::X,     extent.x),
            ScaleAxis::XNeg => (Vec3::NEG_X, extent.x),
            ScaleAxis::YPos => (Vec3::Y,     extent.y),
            ScaleAxis::YNeg => (Vec3::NEG_Y, extent.y),
            ScaleAxis::ZPos => (Vec3::Z,     extent.z),
            ScaleAxis::ZNeg => (Vec3::NEG_Z, extent.z),
            ScaleAxis::Uniform => continue,   // no shaft for center
        };
        t.translation = dir * (face + handle_ext * 0.5);
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
        } else if state.hovered_axis == Some(h.axis) && state.dragged_axis.is_none() {
            // Hover highlight matches the move/rotate gizmos — yellow on
            // the handle the cursor is over, including the central
            // uniform-scale cube.
            AdornmentAxisColor::Hover
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
