//! # Rotate Tool Handles (Mesh-Based, Group-Aware)
//!
//! Singleton `RotateHandleRoot` with 3 torus rings (+ 1 center sphere)
//! positioned at the group AABB center. Structure parallels
//! [`move_handles`](crate::move_handles) and
//! [`scale_handles`](crate::scale_handles).
//!
//! ## Layout in root-local space
//!
//! - Ring X: axis = world X, lies in YZ plane
//! - Ring Y: axis = world Y, lies in XZ plane (torus default)
//! - Ring Z: axis = world Z, lies in XY plane
//! - Center sphere: small pivot indicator at origin
//!
//! ## Sizing
//!
//! Ring radius = `max(group_half_diagonal × 1.15, min_screen_size)`,
//! so tiny parts still get clickable rings. Ring tube thickness
//! scales with radius.
//!
//! ## Transform mode
//!
//! - **World** → `Quat::IDENTITY`, rings around world axes
//! - **Local** → gizmo rotated by the active entity's rotation so
//!   rings wrap around the part's local axes

use bevy::prelude::*;
use eustress_common::adornments::{
    Adornment, CylinderHandleAdornment, SphereHandleAdornment,
};
use crate::adornment_renderer::AdornmentAxisColor;
use crate::selection_box::Selected;
use crate::rotate_tool::RotateToolState;
use crate::move_tool::Axis3d;
use crate::classes::BasePart;
use crate::ui::{StudioState, Tool, TransformMode};
use crate::math_utils::calculate_rotated_aabb;

// ============================================================================
// Components
// ============================================================================

#[derive(Component)]
pub struct RotateHandleRoot {
    pub cached_radius: f32,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RotateRing {
    pub axis: Axis3d,
}

#[derive(Component)]
pub struct RotateCenterPivot;

// ============================================================================
// Constants
// ============================================================================

/// Ring radius = group half-diagonal × this factor, so ring is just
/// outside the bounding sphere. Matches rotate_tool::compute_ring_radius.
const RING_RADIUS_FRAC: f32 = 1.15;
/// Minor (tube) radius as a fraction of major radius.
const RING_MINOR_FRAC: f32 = 0.035;
/// Center sphere radius as a fraction of major radius.
const CENTER_SPHERE_FRAC: f32 = 0.05;
/// Camera-distance floor for readability on tiny selections.
const MIN_SCREEN_FRACTION: f32 = 0.10;

// ============================================================================
// Plugin
// ============================================================================

pub struct RotateHandlesPlugin;

impl Plugin for RotateHandlesPlugin {
    fn build(&self, app: &mut App) {
        // Run BEFORE Bevy's transform propagation so the rotate ring
        // stays glued to the part during a drag. Same rationale as
        // `MoveHandlesPlugin` — see there for the full story.
        app.add_systems(
            PostUpdate,
            (
                sync_rotate_handle_root,
                update_ring_colors,
            )
                .before(bevy::transform::TransformSystems::Propagate),
        );
    }
}

// ============================================================================
// Core system
// ============================================================================

fn sync_rotate_handle_root(
    mut commands: Commands,
    studio_state: Option<Res<StudioState>>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
    // Selected parts: LOCAL `Transform` so the rotate-tool's write
    // this frame is picked up before TransformPropagate runs.
    selected: Query<
        (Entity, &Transform, Option<&BasePart>),
        With<Selected>,
    >,
    // `Without<Selected>` disjoints this from the `selected` read-only
    // Transform query above — same fix as scale_handles / move_handles.
    mut root_query: Query<(&mut RotateHandleRoot, &mut Transform), (Without<RotateRing>, Without<Selected>)>,
    existing_root: Query<Entity, With<RotateHandleRoot>>,
    mut ring_query: Query<
        (&RotateRing, &mut CylinderHandleAdornment),
        (Without<RotateHandleRoot>, Without<RotateCenterPivot>),
    >,
    mut pivot_query: Query<
        &mut SphereHandleAdornment,
        (With<RotateCenterPivot>, Without<RotateHandleRoot>, Without<RotateRing>),
    >,
) {
    let Some(studio_state) = studio_state else { return };

    let tool_active = matches!(studio_state.current_tool, Tool::Rotate);
    let has_selection = !selected.is_empty();

    if !tool_active || !has_selection {
        for e in existing_root.iter() {
            commands.entity(e).despawn();
        }
        return;
    }

    if existing_root.iter().next().is_none() {
        spawn_rotate_handle_root(&mut commands);
        return;
    }

    let Some(group) = compute_group_frame(
        selected.iter().map(|(e, t, bp)| (e, t, bp)),
    ) else {
        return;
    };

    let Some((_, cam_gt, projection)) = cameras.iter().find(|(c, _, _)| c.order == 0) else {
        return;
    };
    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => std::f32::consts::FRAC_PI_4,
    };
    let cam_dist = (group.center - cam_gt.translation()).length().max(0.1);
    let min_screen_size = cam_dist * (fov * 0.5).tan() * MIN_SCREEN_FRACTION;

    let half_diag = group.extent.length();
    let size_radius = half_diag * RING_RADIUS_FRAC;
    let ring_radius = size_radius.max(min_screen_size);
    let ring_minor = ring_radius * RING_MINOR_FRAC;
    let center_radius = ring_radius * CENTER_SPHERE_FRAC;

    let rotation = match studio_state.transform_mode {
        TransformMode::World => Quat::IDENTITY,
        TransformMode::Local => group.active_rotation,
    };

    let Ok((mut root, mut t)) = root_query.single_mut() else { return };
    t.translation = group.center;
    t.rotation = rotation;
    t.scale = Vec3::ONE;

    if (root.cached_radius - ring_radius).abs() > 1e-4 {
        root.cached_radius = ring_radius;

        for (_ring, mut cyl) in ring_query.iter_mut() {
            // CylinderHandleAdornment.height → torus diameter (ring spans
            // ±radius along its lying plane).
            cyl.height = ring_radius * 2.0;
            cyl.radius = ring_minor;
            // inner_radius > 0 signals torus variant to adornment_renderer.
            cyl.inner_radius = (ring_radius - ring_minor).max(0.001);
            cyl.angle = 360.0;
        }

        if let Ok(mut pivot) = pivot_query.single_mut() {
            pivot.radius = center_radius;
        }
    }
}

// ----------------------------------------------------------------------------

struct GroupFrame {
    center: Vec3,
    extent: Vec3,
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
// Spawn
// ============================================================================

fn spawn_rotate_handle_root(commands: &mut Commands) {
    let root = commands.spawn((
        RotateHandleRoot { cached_radius: 0.0 },
        Adornment { meta: true },
        Transform::IDENTITY,
        Visibility::default(),
        Name::new("RotateHandleRoot"),
    )).id();

    // 3 rings. For each, rotate the unit torus (axis = +Y in local) so
    // its axis aligns with the world axis we want.
    let rings = [
        (Axis3d::X, Vec3::X, AdornmentAxisColor::X),
        (Axis3d::Y, Vec3::Y, AdornmentAxisColor::Y),
        (Axis3d::Z, Vec3::Z, AdornmentAxisColor::Z),
    ];

    for (axis, normal, color) in rings {
        let rot = rotation_aligning_y_to(normal);
        commands.spawn((
            CylinderHandleAdornment {
                height: 2.0,              // placeholder, overwritten in sync
                radius: 0.035,
                inner_radius: 0.965,      // non-zero → torus
                angle: 360.0,
                shading: Default::default(),
            },
            color,
            RotateRing { axis },
            Adornment { meta: true },
            Transform { translation: Vec3::ZERO, rotation: rot, scale: Vec3::ONE },
            Visibility::default(),
            Name::new(ring_name(axis)),
            ChildOf(root),
        ));
    }

    // Center pivot sphere — white, small.
    commands.spawn((
        SphereHandleAdornment {
            radius: 0.05,   // placeholder
            shading: Default::default(),
        },
        AdornmentAxisColor::Center,
        RotateCenterPivot,
        Adornment { meta: true },
        Transform::IDENTITY,
        Visibility::default(),
        Name::new("RotateCenterPivot"),
        ChildOf(root),
    ));
}

fn rotation_aligning_y_to(dir: Vec3) -> Quat {
    let d = dir.normalize_or_zero();
    if d.length_squared() < 0.0001 { return Quat::IDENTITY; }
    if d.dot(Vec3::Y) < -0.9999 {
        return Quat::from_axis_angle(Vec3::X, std::f32::consts::PI);
    }
    Quat::from_rotation_arc(Vec3::Y, d)
}

fn ring_name(axis: Axis3d) -> String {
    let s = match axis {
        Axis3d::X => "X",
        Axis3d::Y => "Y",
        Axis3d::Z => "Z",
    };
    format!("RotateRing.{}", s)
}

// ============================================================================
// Color swap
// ============================================================================

fn update_ring_colors(
    state: Option<Res<RotateToolState>>,
    mut rings: Query<(&RotateRing, &mut AdornmentAxisColor)>,
) {
    let Some(state) = state else { return };

    for (ring, mut color) in rings.iter_mut() {
        let desired = if state.dragged_axis == Some(ring.axis) {
            AdornmentAxisColor::Drag
        } else {
            match ring.axis {
                Axis3d::X => AdornmentAxisColor::X,
                Axis3d::Y => AdornmentAxisColor::Y,
                Axis3d::Z => AdornmentAxisColor::Z,
            }
        };
        if *color != desired {
            *color = desired;
        }
    }
}
