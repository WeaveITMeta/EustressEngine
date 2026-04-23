//! # Move Tool Handles (Mesh-Based, Group-Aware)
//!
//! Replaces the old `draw_move_gizmos` function that used Bevy's
//! immediate-mode `Gizmos<TransformGizmoGroup>` (which never rendered
//! through our Slint overlay camera stack). Mesh-based rendering uses
//! the same pipeline as the selection wireframe — which is
//! demonstrably visible through the Slint overlay.
//!
//! ## Singleton root
//!
//! There is **exactly one** `MoveHandleRoot` in the world when the Move
//! tool is active and at least one entity is selected. The root is
//! **not** parented to any adornee — it's a standalone entity whose
//! Transform is recomputed every frame from the current selection:
//!
//! - **Translation**: the group AABB center of all Selected entities.
//!   Single-selection → the entity's world position. Multi-selection →
//!   the midpoint of the combined bounds.
//! - **Rotation**: depends on `StudioState::transform_mode`:
//!   - `World` (default) → `Quat::IDENTITY`, handles axis-aligned to world
//!   - `Local` + single selection → the entity's world rotation
//!   - `Local` + multi selection → the active (last-listed) entity's
//!     world rotation, matching Maya / Unity / Roblox convention
//! - **Scale**: camera-distance formula so handles stay ~constant screen size
//!
//! ## Structure
//!
//! ```text
//! MoveHandleRoot (singleton — not parented)
//!   ├── Shaft(+X)  [CylinderHandleAdornment, AdornmentAxisColor::X]
//!   ├── Tip  (+X)  [ConeHandleAdornment,     AdornmentAxisColor::X]
//!   ├── Shaft(−X), Tip(−X)
//!   ├── Shaft(±Y), Tip(±Y)
//!   └── Shaft(±Z), Tip(±Z)
//! ```
//!
//! Children are in root-local space along canonical axes; root's
//! world transform places them correctly.
//!
//! ## Why singleton vs. per-entity
//!
//! The previous per-entity design spawned N sets of arrows for N
//! selected parts — visible in screenshots as overlapping gizmos with
//! no clear "group center" to drag. Drag math in `move_tool.rs`
//! already operates on group bounds; the VISUAL was out of sync. One
//! root at the group center resolves the visual + interaction mismatch
//! at once.
//!
//! ## Hit detection
//!
//! `move_tool::detect_axis_hit` takes `center`, `handle_len`, and
//! camera — identical inputs to this file's layout math. As long as
//! both files use the same `center` (group AABB) and the same
//! `handle_len` (camera-distance formula), click targets match visible
//! arrows. See §5 Architectural Requirements in TOOLSET.md.

use bevy::prelude::*;
use eustress_common::adornments::{
    Adornment, ConeHandleAdornment, CylinderHandleAdornment,
};
use crate::adornment_renderer::AdornmentAxisColor;
use crate::selection_box::Selected;
use crate::move_tool::{Axis3d, MoveToolState};
use crate::classes::BasePart;
use crate::ui::{StudioState, Tool, TransformMode};
use crate::math_utils::calculate_rotated_aabb;

// ============================================================================
// Components
// ============================================================================

/// Singleton root for the Move tool's 6-arrow handle set. There is at
/// most one of these in the world at any time.
#[derive(Component)]
pub struct MoveHandleRoot;

/// Identifies which axis-direction a shaft or tip entity represents.
/// Used by hover/drag systems to swap color without rebuilding the mesh.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveArrow {
    pub axis: Axis3d,
    /// +1 or −1 along the axis.
    pub sign: i8,
}

/// Identifies a plane-drag handle. `normal_axis` is the axis PERPENDICULAR
/// to the plane — e.g. `normal_axis = Axis3d::Z` → XY-plane handle, drag
/// moves the part in X and Y simultaneously.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovePlaneHandle {
    pub normal_axis: Axis3d,
}

// ============================================================================
// Layout constants — all in the ROOT's local space
// ============================================================================

/// Length of the shaft in root-local units. The root's per-frame scale
/// converts this to world units so arrows stay at constant screen size.
const SHAFT_LEN: f32 = 1.0;
/// Radius of the shaft cylinder. Thin — arrow-like.
const SHAFT_RADIUS: f32 = 0.03;
/// Height of the cone tip (extends BEYOND the shaft end).
const TIP_HEIGHT: f32 = 0.22;
/// Base radius of the cone tip. Wide enough to be clickable.
const TIP_RADIUS: f32 = 0.09;
/// Keep handles at ~8% of screen height in world units.
const SCREEN_FRACTION: f32 = 0.16;

/// Plane handle size in root-local units. Small square at the corner
/// between two axes, far enough from origin that it doesn't crowd the
/// axis shafts.
const PLANE_HANDLE_SIZE: f32 = 0.22;
/// Offset of plane handle center from origin along both non-normal axes.
/// E.g. XY-plane handle sits at (0.3, 0.3, 0) in root-local.
const PLANE_HANDLE_OFFSET: f32 = 0.30;

// ============================================================================
// Plugin
// ============================================================================

pub struct MoveHandlesPlugin;

impl Plugin for MoveHandlesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (
                sync_move_handle_root,   // spawn/despawn + per-frame transform
                update_handle_colors,    // hover/drag color swap
            ),
        );
    }
}

// ============================================================================
// Core system — spawn/despawn and per-frame update in one pass
// ============================================================================

/// Single system that owns the `MoveHandleRoot`'s lifetime and
/// transform. Runs every frame:
///
/// 1. If tool ≠ Move OR no selection → despawn root (if it exists), exit.
/// 2. If root doesn't exist yet → spawn it with 12 arrow children.
/// 3. Compute group AABB center + active-entity rotation.
/// 4. Compute camera-distance screen scale.
/// 5. Write root's Transform.
///
/// Consolidating into one system avoids the previous spawn/despawn
/// race conditions and keeps the singleton invariant obvious.
fn sync_move_handle_root(
    mut commands: Commands,
    studio_state: Option<Res<StudioState>>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
    selected: Query<
        (Entity, &GlobalTransform, Option<&BasePart>),
        With<Selected>,
    >,
    mut root_query: Query<&mut Transform, With<MoveHandleRoot>>,
    existing_root: Query<Entity, With<MoveHandleRoot>>,
) {
    let Some(studio_state) = studio_state else { return };

    // Show Move handles only when Move is the active tool.
    let tool_active = matches!(studio_state.current_tool, Tool::Move);
    let has_selection = !selected.is_empty();

    // Despawn when not applicable.
    if !tool_active || !has_selection {
        for e in existing_root.iter() {
            commands.entity(e).despawn();
        }
        return;
    }

    // Spawn singleton if missing.
    if existing_root.iter().next().is_none() {
        spawn_move_handle_root(&mut commands);
        // Transform gets set next frame when the root component exists —
        // acceptable 1-frame lag on tool activation.
        return;
    }

    // Compute group bounds over ALL selected entities.
    let Some((center, active_rotation)) = compute_group_frame(
        selected.iter().map(|(e, gt, bp)| (e, gt, bp)),
    ) else {
        return;
    };

    // Camera-distance scale so handles stay at constant screen size.
    let Some((_, cam_gt, projection)) = cameras.iter().find(|(c, _, _)| c.order == 0) else {
        return;
    };
    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => std::f32::consts::FRAC_PI_4,
    };
    let dist = (center - cam_gt.translation()).length().max(0.1);
    let screen_scale = dist * (fov * 0.5).tan() * SCREEN_FRACTION;

    // Root rotation depends on transform mode.
    //   World → always identity
    //   Local + 1 entity → that entity's world rotation
    //   Local + N entities → the active (last-listed) entity's rotation
    let rotation = match studio_state.transform_mode {
        TransformMode::World => Quat::IDENTITY,
        TransformMode::Local => active_rotation,
    };

    // Write the root's transform. The follow system guarantees the root
    // is standalone (no ChildOf), so its local Transform IS its world
    // Transform — no parent-scale counter needed.
    if let Ok(mut t) = root_query.single_mut() {
        t.translation = center;
        t.rotation = rotation;
        t.scale = Vec3::splat(screen_scale);
    }
}

/// Compute group-AABB center + "active-entity" rotation from a
/// selection iterator. Returns None if iterator is empty.
///
/// Active entity: the LAST one in the selection iterator. Bevy's query
/// order isn't selection order, but within a single frame it's stable —
/// good enough for the Local-rotation-of-active heuristic. For stricter
/// ordering we'd need a separate "ActiveSelection" resource; deferred
/// to Phase 1.
fn compute_group_frame<'a>(
    iter: impl Iterator<Item = (Entity, &'a GlobalTransform, Option<&'a BasePart>)>,
) -> Option<(Vec3, Quat)> {
    let mut bounds_min = Vec3::splat(f32::MAX);
    let mut bounds_max = Vec3::splat(f32::MIN);
    let mut count = 0;
    let mut last_rotation = Quat::IDENTITY;

    for (_entity, gt, base_part) in iter {
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
    Some((center, last_rotation))
}

// ============================================================================
// Spawn
// ============================================================================

fn spawn_move_handle_root(commands: &mut Commands) {
    let root = commands.spawn((
        MoveHandleRoot,
        Adornment { meta: true },
        Transform::IDENTITY,
        Visibility::default(),
        Name::new("MoveHandleRoot"),
    )).id();

    // Six axes — for each, spawn a cylindrical shaft and a cone tip.
    let axes = [
        (Axis3d::X,  1, AdornmentAxisColor::X),
        (Axis3d::X, -1, AdornmentAxisColor::X),
        (Axis3d::Y,  1, AdornmentAxisColor::Y),
        (Axis3d::Y, -1, AdornmentAxisColor::Y),
        (Axis3d::Z,  1, AdornmentAxisColor::Z),
        (Axis3d::Z, -1, AdornmentAxisColor::Z),
    ];

    for (axis, sign, color) in axes {
        spawn_arrow(commands, root, axis, sign, color);
    }

    // 3 plane handles — one per coordinate plane. normal_axis is the
    // axis perpendicular to the plane; the handle's position is at
    // the midpoint between the two non-normal axes.
    let planes = [
        (Axis3d::Z, Vec3::new( PLANE_HANDLE_OFFSET,  PLANE_HANDLE_OFFSET, 0.0), AdornmentAxisColor::XYPlane),
        (Axis3d::Y, Vec3::new( PLANE_HANDLE_OFFSET,  0.0,                 PLANE_HANDLE_OFFSET), AdornmentAxisColor::XZPlane),
        (Axis3d::X, Vec3::new( 0.0,                  PLANE_HANDLE_OFFSET, PLANE_HANDLE_OFFSET), AdornmentAxisColor::YZPlane),
    ];

    for (normal_axis, local_pos, color) in planes {
        spawn_plane_handle(commands, root, normal_axis, local_pos, color);
    }
}

/// Spawn a small flat box (thin in the plane's normal direction) that
/// represents a 2-axis plane drag handle.
fn spawn_plane_handle(
    commands: &mut Commands,
    root: Entity,
    normal_axis: Axis3d,
    local_pos: Vec3,
    color: AdornmentAxisColor,
) {
    // Box dimensions: square in the plane, very thin along the normal.
    // Thin so it reads as "flat sticker" rather than a cube.
    let (size_x, size_y, size_z) = match normal_axis {
        Axis3d::X => (0.01,               PLANE_HANDLE_SIZE, PLANE_HANDLE_SIZE),
        Axis3d::Y => (PLANE_HANDLE_SIZE, 0.01,               PLANE_HANDLE_SIZE),
        Axis3d::Z => (PLANE_HANDLE_SIZE, PLANE_HANDLE_SIZE, 0.01),
    };

    commands.spawn((
        eustress_common::adornments::BoxHandleAdornment {
            size: Vec3::new(size_x, size_y, size_z),
            shading: Default::default(),
        },
        color,
        MovePlaneHandle { normal_axis },
        Adornment { meta: true },
        Transform {
            translation: local_pos,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        },
        Visibility::default(),
        Name::new(format!("MovePlane.{:?}", normal_axis)),
        ChildOf(root),
    ));
}

fn spawn_arrow(
    commands: &mut Commands,
    root: Entity,
    axis: Axis3d,
    sign: i8,
    color: AdornmentAxisColor,
) {
    let dir: Vec3 = axis.to_vec3() * (sign as f32);
    let rot = rotation_aligning_y_to(dir);

    // Shaft: cylinder spanning origin → dir × SHAFT_LEN.
    let shaft_midpoint = dir * (SHAFT_LEN * 0.5);
    commands.spawn((
        CylinderHandleAdornment {
            height: SHAFT_LEN,
            radius: SHAFT_RADIUS,
            inner_radius: 0.0,
            angle: 360.0,
            shading: Default::default(),
        },
        color,
        MoveArrow { axis, sign },
        Adornment { meta: true },
        Transform {
            translation: shaft_midpoint,
            rotation: rot,
            scale: Vec3::ONE,
        },
        Visibility::default(),
        Name::new(arrow_name("Shaft", axis, sign)),
        ChildOf(root),
    ));

    // Tip: cone sitting ON the end of the shaft, pointing outward.
    let tip_center = dir * (SHAFT_LEN + TIP_HEIGHT * 0.5);
    commands.spawn((
        ConeHandleAdornment {
            height: TIP_HEIGHT,
            radius: TIP_RADIUS,
            shading: Default::default(),
        },
        color,
        MoveArrow { axis, sign },
        Adornment { meta: true },
        Transform {
            translation: tip_center,
            rotation: rot,
            scale: Vec3::ONE,
        },
        Visibility::default(),
        Name::new(arrow_name("Tip", axis, sign)),
        ChildOf(root),
    ));
}

/// Rotation quaternion that rotates local `+Y` onto the given direction.
fn rotation_aligning_y_to(dir: Vec3) -> Quat {
    let d = dir.normalize_or_zero();
    if d.length_squared() < 0.0001 { return Quat::IDENTITY; }
    if d.dot(Vec3::Y) < -0.9999 {
        return Quat::from_axis_angle(Vec3::X, std::f32::consts::PI);
    }
    Quat::from_rotation_arc(Vec3::Y, d)
}

fn arrow_name(kind: &str, axis: Axis3d, sign: i8) -> String {
    let axis_s = match axis {
        Axis3d::X => "X",
        Axis3d::Y => "Y",
        Axis3d::Z => "Z",
    };
    let sign_s = if sign > 0 { "+" } else { "-" };
    format!("MoveArrow.{}{}.{}", sign_s, axis_s, kind)
}

// ============================================================================
// Hover/drag color swap
// ============================================================================

fn update_handle_colors(
    move_state: Option<Res<MoveToolState>>,
    mut arrows: Query<(&MoveArrow, &mut AdornmentAxisColor), Without<MovePlaneHandle>>,
    mut planes: Query<(&MovePlaneHandle, &mut AdornmentAxisColor), Without<MoveArrow>>,
) {
    let Some(move_state) = move_state else { return };

    // Axis arrows
    for (arrow, mut color) in arrows.iter_mut() {
        let desired = if move_state.dragged_axis == Some(arrow.axis) {
            AdornmentAxisColor::Drag
        } else if move_state.hovered_axis == Some(arrow.axis)
            && move_state.dragged_axis.is_none()
            && move_state.dragged_plane.is_none()
        {
            AdornmentAxisColor::Hover
        } else {
            match arrow.axis {
                Axis3d::X => AdornmentAxisColor::X,
                Axis3d::Y => AdornmentAxisColor::Y,
                Axis3d::Z => AdornmentAxisColor::Z,
            }
        };
        if *color != desired {
            *color = desired;
        }
    }

    // Plane handles — own color maps to their plane type.
    for (plane, mut color) in planes.iter_mut() {
        let base = match plane.normal_axis {
            Axis3d::Z => AdornmentAxisColor::XYPlane,  // normal=Z → XY plane
            Axis3d::Y => AdornmentAxisColor::XZPlane,  // normal=Y → XZ plane
            Axis3d::X => AdornmentAxisColor::YZPlane,  // normal=X → YZ plane
        };
        let desired = if move_state.dragged_plane == Some(plane.normal_axis) {
            AdornmentAxisColor::Drag
        } else if move_state.hovered_plane == Some(plane.normal_axis)
            && move_state.dragged_axis.is_none()
            && move_state.dragged_plane.is_none()
        {
            AdornmentAxisColor::Hover
        } else {
            base
        };
        if *color != desired {
            *color = desired;
        }
    }
}
