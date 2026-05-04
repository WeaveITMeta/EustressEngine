// ============================================================================
// Eustress Engine - Scale Tool
// ============================================================================
// ## Table of Contents
// 1. State & types
// 2. Plugin registration
// 3. Gizmo drawing (cube handles at face centers, camera-scaled)
// 4. Mouse interaction (per-axis and symmetric scaling)
// 5. Public helpers
// ============================================================================

#![allow(dead_code)]
#![allow(unused_variables)]

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::selection_box::Selected;
use crate::gizmo_tools::TransformGizmoGroup;
use crate::math_utils::{ray_plane_intersection, ray_to_point_distance};
use crate::move_tool::Axis3d;

// ============================================================================
// 1. State & Types
// ============================================================================

#[derive(Resource, Default)]
pub struct ScaleToolState {
    pub active: bool,
    pub dragged_axis: Option<ScaleAxis>,
    /// Axis the cursor is currently hovering over (when not dragging).
    /// Drives the gizmo's hover-color swap so the user gets immediate
    /// feedback that a handle is hot-spot.
    pub hovered_axis: Option<ScaleAxis>,
    pub initial_scale: Vec3,
    pub initial_position: Vec3,
    pub drag_start_pos: Vec2,
    pub initial_mouse_world: Vec3,
    pub dragged_entity: Option<Entity>,
    pub initial_scales: std::collections::HashMap<Entity, Vec3>,
    pub initial_positions: std::collections::HashMap<Entity, Vec3>,
    pub group_center: Vec3,
    /// Gizmo rotation captured at drag start. `IDENTITY` for World mode,
    /// active entity's rotation for Local. Unused in current drag math
    /// (which is camera-relative) but preserved for future numeric-input
    /// + parametric-scale features where the frame matters.
    pub drag_rotation: Quat,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum ScaleAxis {
    XPos, XNeg,
    YPos, YNeg,
    ZPos, ZNeg,
    Uniform,
}

impl ScaleAxis {
    pub fn axis(self) -> Axis3d {
        match self {
            ScaleAxis::XPos | ScaleAxis::XNeg => Axis3d::X,
            ScaleAxis::YPos | ScaleAxis::YNeg => Axis3d::Y,
            ScaleAxis::ZPos | ScaleAxis::ZNeg => Axis3d::Z,
            ScaleAxis::Uniform => Axis3d::Y,
        }
    }

    fn sign(self) -> f32 {
        match self {
            ScaleAxis::XPos | ScaleAxis::YPos | ScaleAxis::ZPos | ScaleAxis::Uniform => 1.0,
            ScaleAxis::XNeg | ScaleAxis::YNeg | ScaleAxis::ZNeg => -1.0,
        }
    }

    fn color(self) -> Color {
        match self {
            ScaleAxis::XPos | ScaleAxis::XNeg => Color::srgb(0.95, 0.15, 0.15),
            ScaleAxis::YPos | ScaleAxis::YNeg => Color::srgb(0.15, 0.95, 0.15),
            ScaleAxis::ZPos | ScaleAxis::ZNeg => Color::srgb(0.15, 0.15, 0.95),
            ScaleAxis::Uniform => Color::srgb(1.0, 1.0, 1.0),
        }
    }
}

// ============================================================================
// 2. Plugin Registration
// ============================================================================

pub struct ScaleToolPlugin;

/// Absolute-resize event — fire this to set a part's `BasePart.size`
/// to an exact vector. The handler reuses the same `apply_size_to_entity`
/// path the scale-gizmo + numeric-input commit use, so primitive-mesh
/// regeneration, custom-GLB `Transform.scale` mode, and `BasePart.cframe`
/// bookkeeping all stay consistent.
///
/// Emitted by:
/// * The Properties-panel `"Size"` copy/paste write-back — without
///   this, pasting a size would update `Transform.scale` only (which
///   parts keep at `[1, 1, 1]`) and leave the visible dimensions
///   unchanged.
/// * Any future tool that wants to resize a part by an absolute
///   value (e.g. a paste-props MCP action).
#[derive(bevy::prelude::Message, Debug, Clone)]
pub struct ResizePartEvent {
    pub entity: Entity,
    pub new_size: Vec3,
}

impl Plugin for ScaleToolPlugin {
    fn build(&self, app: &mut App) {
        // Gizmo drawing moved to `scale_handles::ScaleHandlesPlugin`
        // (mesh-based, renders through the same pipeline as the
        // selection wireframe instead of Bevy's gizmo graph).
        app.init_resource::<ScaleToolState>()
            .add_message::<ResizePartEvent>()
            .add_systems(Update, (
                handle_scale_interaction,
                // Numeric-input commit — applies typed size exactly and
                // finalizes the drag. Runs after cursor-driven drag so
                // Enter wins over any in-progress cursor delta.
                finalize_numeric_input_on_scale.after(handle_scale_interaction),
                // Absolute-resize handler for `ResizePartEvent` (emitted
                // by the Properties-panel paste path + future tools).
                handle_resize_part_events,
                // Rebuild the Avian collider whenever `BasePart.size`
                // changes. Runs after resize events so the collider
                // update picks up the final size in the same frame.
                rebuild_collider_on_size_change.after(handle_resize_part_events),
            ));
    }
}

/// Rebuild the Avian `Collider` in-place whenever an entity's
/// `BasePart.size` changes — scale-tool drag, Properties-panel type-in,
/// paste-props, MCP resize, undo/redo, any write-back from disk. Without
/// this, the visual mesh resized but the collider stayed at the spawn
/// dimensions, so raycasts, surface snapping, selection hit-test, and
/// physics all stepped into "shadow-of-the-old-size" territory.
///
/// Only runs for entities that already have a `Collider` — we never
/// add physics to a part that was spawned without `can_collide`.
fn rebuild_collider_on_size_change(
    mut commands: Commands,
    changed: Query<
        (Entity, &crate::classes::BasePart, Option<&crate::classes::Part>),
        (Changed<crate::classes::BasePart>, With<avian3d::prelude::Collider>),
    >,
) {
    use avian3d::prelude::Collider;
    use crate::classes::PartType;

    for (entity, base_part, part_opt) in changed.iter() {
        // Sanitise dimensions the same way the scale-tool does — a
        // degenerate 0 / negative / non-finite size would panic Avian's
        // collider builder on the next physics step.
        let half = Vec3::new(
            if base_part.size.x.is_finite() { (base_part.size.x * 0.5).abs().max(0.05) } else { 0.05 },
            if base_part.size.y.is_finite() { (base_part.size.y * 0.5).abs().max(0.05) } else { 0.05 },
            if base_part.size.z.is_finite() { (base_part.size.z * 0.5).abs().max(0.05) } else { 0.05 },
        );
        let collider = match part_opt.map(|p| p.shape) {
            Some(PartType::Ball) => Collider::sphere(half.x),
            Some(PartType::Cylinder) | Some(PartType::Cone) => {
                Collider::cylinder(half.x, half.y)
            }
            _ => Collider::cuboid(half.x, half.y, half.z),
        };
        commands.entity(entity).insert(collider);
    }
}

/// Drain `ResizePartEvent`s emitted by the Properties-panel paste
/// handler (etc.) and apply each via the shared `apply_size_to_entity`
/// helper so primitive mesh regen + transform adjustment stay in one
/// code path with the scale-gizmo commit.
fn handle_resize_part_events(
    mut events: MessageReader<ResizePartEvent>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut entities: Query<(
        &mut Transform,
        Option<&mut crate::classes::BasePart>,
        Option<&crate::classes::Part>,
        Option<&mut Mesh3d>,
        Option<&crate::spawn::MeshSource>,
    )>,
) {
    for event in events.read() {
        let Ok((mut transform, base_part, part, mesh, mesh_source)) = entities.get_mut(event.entity)
        else { continue };
        let has_mesh_source = mesh_source.is_some();
        let pos = transform.translation;
        // One-shot resize (Properties paste-write): bake the mesh now,
        // no in-progress drag to defer to. mesh_baked_size unused.
        apply_size_to_entity(
            &mut *transform,
            base_part,
            part,
            mesh,
            &mut meshes,
            event.new_size,
            pos,
            has_mesh_source,
            true,
            Vec3::ONE,
        );
    }
}

// ============================================================================
// 3. Gizmo Drawing
// ============================================================================

fn draw_scale_gizmos(
    mut gizmos: Gizmos<TransformGizmoGroup>,
    state: Res<ScaleToolState>,
    query: Query<(&GlobalTransform, Option<&crate::classes::BasePart>), With<Selected>>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
) {
    if !state.active || query.is_empty() { return; }

    let Some((_, cam_gt, projection)) = cameras.iter().find(|(c, _, _)| c.order == 0) else { return };
    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => std::f32::consts::FRAC_PI_4,
    };

    let yellow = Color::srgb(1.0, 1.0, 0.0);

    for (global_transform, base_part) in &query {
        let t = global_transform.compute_transform();
        let pos = t.translation;
        let rot = t.rotation;
        let size = base_part.map(|bp| bp.size).unwrap_or(t.scale);

        // Camera-distance-scaled handle length
        let dist = (pos - cam_gt.translation()).length().max(0.1);
        let scale = dist * (fov * 0.5).tan() * 0.16;
        let handle_len = scale * 0.9;
        let cube_size  = scale * 0.18;

        let local_x = rot * Vec3::X;
        let local_y = rot * Vec3::Y;
        let local_z = rot * Vec3::Z;

        // Handle origins at face centers
        let face_x_pos = pos + local_x * (size.x * 0.5);
        let face_x_neg = pos - local_x * (size.x * 0.5);
        let face_y_pos = pos + local_y * (size.y * 0.5);
        let face_y_neg = pos - local_y * (size.y * 0.5);
        let face_z_pos = pos + local_z * (size.z * 0.5);
        let face_z_neg = pos - local_z * (size.z * 0.5);

        let hl = |ax: ScaleAxis| if state.dragged_axis == Some(ax) { yellow } else { ax.color() };

        // X axis handles
        let x_tip_pos = face_x_pos + local_x * handle_len;
        let x_tip_neg = face_x_neg - local_x * handle_len;
        gizmos.line(face_x_pos, x_tip_pos, hl(ScaleAxis::XPos));
        draw_handle_cube(&mut gizmos, x_tip_pos, rot, cube_size, hl(ScaleAxis::XPos));
        gizmos.line(face_x_neg, x_tip_neg, hl(ScaleAxis::XNeg));
        draw_handle_cube(&mut gizmos, x_tip_neg, rot, cube_size, hl(ScaleAxis::XNeg));

        // Y axis handles
        let y_tip_pos = face_y_pos + local_y * handle_len;
        let y_tip_neg = face_y_neg - local_y * handle_len;
        gizmos.line(face_y_pos, y_tip_pos, hl(ScaleAxis::YPos));
        draw_handle_cube(&mut gizmos, y_tip_pos, rot, cube_size, hl(ScaleAxis::YPos));
        gizmos.line(face_y_neg, y_tip_neg, hl(ScaleAxis::YNeg));
        draw_handle_cube(&mut gizmos, y_tip_neg, rot, cube_size, hl(ScaleAxis::YNeg));

        // Z axis handles
        let z_tip_pos = face_z_pos + local_z * handle_len;
        let z_tip_neg = face_z_neg - local_z * handle_len;
        gizmos.line(face_z_pos, z_tip_pos, hl(ScaleAxis::ZPos));
        draw_handle_cube(&mut gizmos, z_tip_pos, rot, cube_size, hl(ScaleAxis::ZPos));
        gizmos.line(face_z_neg, z_tip_neg, hl(ScaleAxis::ZNeg));
        draw_handle_cube(&mut gizmos, z_tip_neg, rot, cube_size, hl(ScaleAxis::ZNeg));

        // Center uniform-scale cube (white)
        draw_handle_cube(&mut gizmos, pos, rot, cube_size * 1.3,
            if state.dragged_axis == Some(ScaleAxis::Uniform) { yellow }
            else { Color::srgba(1.0, 1.0, 1.0, 0.8) });
    }
}

/// Draw a small wireframe cube at `center` oriented by `rot`.
fn draw_handle_cube(
    gizmos: &mut Gizmos<TransformGizmoGroup>,
    center: Vec3,
    rot: Quat,
    half: f32,
    color: Color,
) {
    let corners = [
        Vec3::new(-half, -half, -half), Vec3::new( half, -half, -half),
        Vec3::new(-half,  half, -half), Vec3::new( half,  half, -half),
        Vec3::new(-half, -half,  half), Vec3::new( half, -half,  half),
        Vec3::new(-half,  half,  half), Vec3::new( half,  half,  half),
    ];
    let wc: Vec<Vec3> = corners.iter().map(|&c| center + rot * c).collect();
    // Bottom
    gizmos.line(wc[0], wc[1], color); gizmos.line(wc[4], wc[5], color);
    gizmos.line(wc[0], wc[4], color); gizmos.line(wc[1], wc[5], color);
    // Top
    gizmos.line(wc[2], wc[3], color); gizmos.line(wc[6], wc[7], color);
    gizmos.line(wc[2], wc[6], color); gizmos.line(wc[3], wc[7], color);
    // Verticals
    gizmos.line(wc[0], wc[2], color); gizmos.line(wc[1], wc[3], color);
    gizmos.line(wc[4], wc[6], color); gizmos.line(wc[5], wc[7], color);
}

// ============================================================================
// 4. Mouse Interaction
// ============================================================================

fn handle_scale_interaction(
    mut state: ResMut<ScaleToolState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
    mut query: Query<(Entity, &GlobalTransform, &mut Transform, Option<&mut crate::classes::BasePart>, Option<&crate::classes::Part>, Option<&mut Mesh3d>, Option<&crate::spawn::MeshSource>), With<Selected>>,
    editor_settings: Res<crate::editor_settings::EditorSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    parent_query: Query<&ChildOf>,
    mut undo_stack: ResMut<crate::undo::UndoStack>,
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
    studio_state: Option<Res<crate::ui::StudioState>>,
) {
    if !state.active {
        // Clear stale hover state so the gizmo doesn't briefly flash a
        // hover color on the first frame after the scale tool is
        // re-activated.
        if state.hovered_axis.is_some() {
            state.hovered_axis = None;
        }
        return;
    }

    // Transform mode governs whether scale handles are axis-aligned to
    // world (World mode) or rotated to match the active entity (Local).
    // Hit test must use the same rotation as `scale_handles::sync_scale_handle_root`
    // renders — otherwise clicking the rotated Local-mode cube misses.
    let transform_mode = studio_state
        .as_ref()
        .map(|s| s.transform_mode)
        .unwrap_or(crate::ui::TransformMode::World);

    // Escape cancels an in-progress scale and restores pre-drag sizes.
    if keys.just_pressed(KeyCode::Escape) {
        if state.dragged_axis.is_some() {
            for (entity, _, mut transform, basepart_opt, _, _, _) in query.iter_mut() {
                if let Some(initial_size) = state.initial_scales.get(&entity).copied() {
                    if let Some(mut bp) = basepart_opt {
                        bp.size = initial_size;
                    }
                }
                if let Some(initial_pos) = state.initial_positions.get(&entity).copied() {
                    transform.translation = initial_pos;
                }
            }
            state.dragged_axis = None;
            state.dragged_entity = None;
            state.initial_scales.clear();
            state.initial_positions.clear();
            return;
        }
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };
    
    // Block NEW drags when cursor is over UI panels (outside 3D viewport).
    // Allow in-progress drags to continue even if cursor leaves the viewport.
    // ViewportBounds is physical px, cursor_pos is logical — go through
    // contains_logical so DPI-scaled displays don't reject every click.
    if state.dragged_axis.is_none() {
        if let Some(vb) = viewport_bounds.as_deref() {
            let scale = window.scale_factor() as f32;
            if !vb.contains_logical(cursor_pos, scale) { return; }
        }
    }
    let Some((camera, camera_transform, projection)) = cameras.iter().find(|(c, _, _)| c.order == 0) else { return };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else { return };

    let camera_forward = camera_transform.forward().as_vec3();
    let camera_right   = camera_transform.right().as_vec3();
    let camera_up      = camera_transform.up().as_vec3();

    let ctrl_pressed = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);

    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => std::f32::consts::FRAC_PI_4,
    };

    // Snapshot the selected entities so we can re-use the same data
    // for hover detection and click-to-drag without re-iterating the
    // query (and to side-step Bevy's `iter().map().clone()` constraints).
    let selected_snapshot: Vec<(Vec3, Quat, Vec3)> = query
        .iter()
        .map(|(_, gt, _, bp, _, _, _)| {
            let t = gt.compute_transform();
            let size = bp.as_ref().map(|b| b.size).unwrap_or(t.scale);
            (t.translation, t.rotation, size)
        })
        .collect();

    // Group-aware hit test for the scale handles. Used both for hover
    // highlighting (every frame) and for click-to-drag.
    let pick = pick_scale_handle(
        ray.origin,
        *ray.direction,
        camera_transform.translation(),
        fov,
        transform_mode,
        &selected_snapshot,
    );

    // Update hover state every frame when not dragging — drives the
    // yellow color swap on the scale gizmo handles.
    if state.dragged_axis.is_none() {
        let new_hover = pick.map(|(axis, _, _, _)| axis);
        if state.hovered_axis != new_hover {
            state.hovered_axis = new_hover;
        }
    }

    if mouse.just_pressed(MouseButton::Left) {
        if let Some((axis, group_center, _group_extent, rotation)) = pick {
            state.dragged_axis = Some(axis);
            // Store drag rotation for consistency with dragged delta math.
            // Unused inside scale_tool today (drag is camera-relative), but
            // future extensions (numeric input, undo label) may need it.
            state.drag_rotation = rotation;
            state.drag_start_pos = cursor_pos;
            state.initial_mouse_world = ray.origin;

            state.initial_scales.clear();
            state.initial_positions.clear();
            for (ent, _, trans, bp_opt, _, _, _) in query.iter() {
                let ent_size = bp_opt.as_ref().map(|bp| bp.size).unwrap_or(Vec3::ONE);
                state.initial_scales.insert(ent, ent_size);
                state.initial_positions.insert(ent, trans.translation);
            }
            state.group_center = group_center;

            if let Some(t) = ray_plane_intersection(ray.origin, *ray.direction, group_center, Vec3::Y) {
                state.initial_mouse_world = ray.origin + *ray.direction * t;
            }
        }
    } else if mouse.pressed(MouseButton::Left) {
        if let Some(axis) = state.dragged_axis {
            let delta_screen = cursor_pos - state.drag_start_pos;
            let drag_distance = delta_screen.length();
            let base_sensitivity = 0.015;
            let progressive_factor = 1.0 + drag_distance * 0.002;
            let sensitivity = base_sensitivity * progressive_factor;

            // Project the screen-space cursor delta onto the GIZMO's
            // world-space axis (not the world's X/Y/Z axis). The gizmo
            // frame is `drag_rotation` captured at click — IDENTITY in
            // World mode, active entity rotation in Local mode — so a
            // rotated part's Local-mode gizmo no longer produces an
            // inverted drag when its +X points toward world -X, and a
            // World-mode gizmo always scales along the world axes the
            // user is visually grabbing.
            let gizmo_x = state.drag_rotation * Vec3::X;
            let gizmo_y = state.drag_rotation * Vec3::Y;
            let gizmo_z = state.drag_rotation * Vec3::Z;
            // Project a world-space gizmo axis onto the screen plane
            // and dot it with the cursor delta. Returns a signed scalar
            // proportional to "how much the user dragged in the
            // direction the handle visibly moved on screen". Cursor
            // moving with the handle's screen-space direction grows
            // the axis; opposite direction shrinks.
            //
            // Earlier this function only consulted `camera_right` and
            // `camera_forward`, never `camera_up`. For the Y axis at a
            // typical viewing angle (camera looking slightly down)
            // both `right.dot(Y)` and `fwd.dot(Y)` are near zero, the
            // heuristic fell into the `fwd.signum()` branch and
            // produced the WRONG sign — dragging the YPos handle up
            // shrank the part toward the YPos face (the "scales the
            // opposite end" symptom). Using `camera_up` (negated for
            // screen y, which is down-positive in cursor coords)
            // captures the vertical screen direction correctly for
            // the world-Y axis regardless of camera tilt.
            let project = |axis_world: Vec3| -> f32 {
                // Screen-space direction of the gizmo axis: the X
                // component is `camera_right · axis`, the Y component
                // is `-camera_up · axis` (cursor Y is inverted from
                // world up).
                let sx = camera_right.dot(axis_world);
                let sy = -camera_up.dot(axis_world);
                let mag_sq = sx * sx + sy * sy;
                if mag_sq < 0.0025 {
                    // Axis is near-perpendicular to the screen plane
                    // (e.g. world-Y when looking straight down). Fall
                    // back to a forward-signed combined-delta so a
                    // top-down user can still scale by dragging
                    // toward / away from the camera.
                    let fwd_sign = -camera_forward.dot(axis_world);
                    return (delta_screen.x - delta_screen.y) * sensitivity * fwd_sign.signum();
                }
                let mag = mag_sq.sqrt();
                (delta_screen.x * sx + delta_screen.y * sy) / mag * sensitivity
            };

            let drag_amount = match axis {
                ScaleAxis::YPos | ScaleAxis::YNeg => project(gizmo_y),
                ScaleAxis::XPos | ScaleAxis::XNeg => project(gizmo_x),
                ScaleAxis::ZPos | ScaleAxis::ZNeg => project(gizmo_z),
                ScaleAxis::Uniform => (delta_screen.x - delta_screen.y) * sensitivity * 0.5,
            };

            let direction_mult = match axis {
                ScaleAxis::XNeg | ScaleAxis::YNeg | ScaleAxis::ZNeg => -1.0,
                _ => 1.0,
            };
            let effective_drag = drag_amount * direction_mult;

            let selected_entities: std::collections::HashSet<Entity> = query.iter().map(|(e, ..)| e).collect();

            for (entity, global_transform, mut transform, basepart_opt, part_opt, mesh_opt, mesh_source) in query.iter_mut() {
                if is_descendant(entity, &selected_entities, &parent_query) { continue; }

                if let (Some(initial_size), Some(initial_pos)) = (
                    state.initial_scales.get(&entity),
                    state.initial_positions.get(&entity),
                ) {
                    // Phase-2 Scale Lock — treat face drags as uniform
                    // when the setting is on. Preserves axis ratios.
                    let effective_axis = if editor_settings.scale_lock_proportional
                        && axis != ScaleAxis::Uniform
                    {
                        ScaleAxis::Uniform
                    } else {
                        axis
                    };
                    let new_size = compute_new_size(effective_axis, *initial_size, effective_drag);
                    let final_size = apply_snap(new_size, &editor_settings);
                    let has_mesh_source = mesh_source.is_some();

                    if ctrl_pressed {
                        // Symmetric: position stays centered. Mid-drag —
                        // skip mesh regen, use Transform.scale instead.
                        // initial_size is the legacy mesh's baked size
                        // (the part hasn't been resized yet this drag).
                        apply_size_to_entity(
                            &mut transform, basepart_opt, part_opt, mesh_opt,
                            &mut meshes, final_size, *initial_pos, has_mesh_source,
                            false, *initial_size,
                        );
                    } else {
                        // One-sided: opposite face (in GIZMO frame) stays
                        // fixed. Using `state.drag_rotation` means:
                        //   * World mode (drag_rotation = IDENTITY) → the
                        //     world-axis face opposite the grabbed handle
                        //     stays anchored, so grabbing world +X moves
                        //     the world +X face and world -X stays put —
                        //     regardless of the part's own rotation. This
                        //     closes the "World mode resizes the opposite
                        //     end" bug: the old code used
                        //     `transform.rotation` which pointed the
                        //     offset along the ENTITY's X axis, not the
                        //     gizmo's world X.
                        //   * Local mode (drag_rotation = entity rotation)
                        //     → identical result to the previous code,
                        //     since both produce the same world vector.
                        let size_diff = final_size - *initial_size;
                        let gizmo_offset = match axis {
                            ScaleAxis::XPos => Vec3::X   * size_diff.x * 0.5,
                            ScaleAxis::XNeg => Vec3::NEG_X * size_diff.x * 0.5,
                            ScaleAxis::YPos => Vec3::Y   * size_diff.y * 0.5,
                            ScaleAxis::YNeg => Vec3::NEG_Y * size_diff.y * 0.5,
                            ScaleAxis::ZPos => Vec3::Z   * size_diff.z * 0.5,
                            ScaleAxis::ZNeg => Vec3::NEG_Z * size_diff.z * 0.5,
                            ScaleAxis::Uniform => Vec3::ZERO,
                        };
                        let world_offset = state.drag_rotation * gizmo_offset;
                        let new_pos = *initial_pos + world_offset;
                        // Mid-drag: defer mesh regen.
                        apply_size_to_entity(
                            &mut transform, basepart_opt, part_opt, mesh_opt,
                            &mut meshes, final_size, new_pos, has_mesh_source,
                            false, *initial_size,
                        );
                    }
                }
            }
        }
    } else if mouse.just_released(MouseButton::Left) {
        if state.dragged_axis.is_some() && !state.initial_scales.is_empty() {
            let mut old_states: Vec<(u64, [f32; 3], [f32; 3])> = Vec::new();
            let mut new_states: Vec<(u64, [f32; 3], [f32; 3])> = Vec::new();

            // First pass: bake the legacy primitive mesh at the final
            // size + restore Transform.scale = ONE. During the drag we
            // deferred regen and used Transform.scale = size/baked
            // for performance — now is the moment to settle that into
            // a real mesh so save_space + selection adornments see the
            // correct geometry. File-system parts (has_mesh_source =
            // true) keep Transform.scale = size and skip this branch.
            for (entity, _, mut transform, basepart_opt, part_opt, mesh_opt, mesh_source) in query.iter_mut() {
                let Some(initial_pos)  = state.initial_positions.get(&entity).copied() else { continue };
                let Some(initial_size) = state.initial_scales.get(&entity).copied() else { continue };
                let has_mesh_source = mesh_source.is_some();
                let final_size = basepart_opt.as_ref().map(|bp| bp.size).unwrap_or(initial_size);
                let final_pos  = transform.translation;
                let size_changed = (initial_size - final_size).length() > 0.001;
                if !size_changed { continue; }
                apply_size_to_entity(
                    &mut *transform, basepart_opt, part_opt, mesh_opt,
                    &mut meshes, final_size, final_pos, has_mesh_source,
                    true, initial_size,
                );
            }

            for (entity, _, transform, basepart_opt, _, _, _) in query.iter() {
                if let (Some(initial_pos), Some(initial_size)) = (
                    state.initial_positions.get(&entity),
                    state.initial_scales.get(&entity),
                ) {
                    let new_size = basepart_opt.as_ref().map(|bp| bp.size).unwrap_or(*initial_size);
                    let pos_changed = (*initial_pos - transform.translation).length() > 0.001;
                    let size_changed = (*initial_size - new_size).length() > 0.001;
                    if pos_changed || size_changed {
                        old_states.push((entity.to_bits(), initial_pos.to_array(), initial_size.to_array()));
                        new_states.push((entity.to_bits(), transform.translation.to_array(), new_size.to_array()));
                    }
                }
            }

            if !old_states.is_empty() {
                undo_stack.push(crate::undo::Action::ScaleEntities { old_states, new_states });
            }
        }

        state.dragged_axis = None;
        state.dragged_entity = None;
        state.initial_scales.clear();
        state.initial_positions.clear();
    }
}

/// Apply a `NumericInputCommittedEvent` to an in-progress Scale drag.
///
/// Semantics of the typed value:
/// - **Uniform** axis (center handle): `value` is a uniform multiplier
///   (relative = `initial * (1 + value)`; absolute = `initial * value`).
/// - **Per-axis face handle** (XPos/XNeg/YPos/YNeg/ZPos/ZNeg): `value`
///   is the absolute target size along that axis (relative = delta).
///   One-sided scale keeps the opposite face fixed — same as the
///   cursor-driven drag code path.
///
/// Matches the existing mouse-release in that it pushes a `ScaleEntities`
/// undo action; TOML persistence for Scale is handled elsewhere and is
/// not duplicated here.
#[allow(clippy::too_many_arguments)]
fn finalize_numeric_input_on_scale(
    mut committed: MessageReader<crate::numeric_input::NumericInputCommittedEvent>,
    mut state: ResMut<ScaleToolState>,
    mut query: Query<(
        Entity,
        &mut Transform,
        Option<&mut crate::classes::BasePart>,
    ), With<Selected>>,
    parent_query: Query<&ChildOf>,
    mut undo_stack: ResMut<crate::undo::UndoStack>,
) {
    use crate::numeric_input::NumericInputOwner;

    for event in committed.read() {
        if event.owner != NumericInputOwner::Scale { continue; }
        let Some(axis) = state.dragged_axis else { continue; };
        if state.initial_scales.is_empty() { continue; }

        let value = event.value;
        let relative = event.relative;

        let selected_set: std::collections::HashSet<Entity> = query.iter().map(|(e, ..)| e).collect();

        let mut old_states: Vec<(u64, [f32; 3], [f32; 3])> = Vec::new();
        let mut new_states: Vec<(u64, [f32; 3], [f32; 3])> = Vec::new();

        for (entity, mut transform, basepart_opt) in query.iter_mut() {
            if is_descendant(entity, &selected_set, &parent_query) { continue; }
            let Some(initial_size) = state.initial_scales.get(&entity).copied() else { continue };
            let Some(initial_pos)  = state.initial_positions.get(&entity).copied() else { continue };

            let new_size = match axis {
                ScaleAxis::Uniform => {
                    let factor = if relative { 1.0 + value } else { value };
                    (initial_size * factor).max(Vec3::splat(0.1))
                }
                _ => {
                    // Per-axis absolute-or-relative. Component picked
                    // by the axis; other two stay.
                    let component = match axis {
                        ScaleAxis::XPos | ScaleAxis::XNeg => initial_size.x,
                        ScaleAxis::YPos | ScaleAxis::YNeg => initial_size.y,
                        ScaleAxis::ZPos | ScaleAxis::ZNeg => initial_size.z,
                        ScaleAxis::Uniform => unreachable!(),
                    };
                    let target = if relative { component + value } else { value };
                    let target = target.max(0.1);
                    match axis {
                        ScaleAxis::XPos | ScaleAxis::XNeg => Vec3::new(target, initial_size.y, initial_size.z),
                        ScaleAxis::YPos | ScaleAxis::YNeg => Vec3::new(initial_size.x, target, initial_size.z),
                        ScaleAxis::ZPos | ScaleAxis::ZNeg => Vec3::new(initial_size.x, initial_size.y, target),
                        ScaleAxis::Uniform => unreachable!(),
                    }
                }
            };

            // One-sided scale — opposite face (in GIZMO frame) stays
            // fixed. Uses `state.drag_rotation` so World-mode numeric
            // commits anchor the world-opposite face and Local-mode
            // commits anchor the entity-opposite face — same invariant
            // as the cursor drag, fixed in the same commit.
            let size_diff = new_size - initial_size;
            let gizmo_offset = match axis {
                ScaleAxis::XPos    => Vec3::X     * size_diff.x * 0.5,
                ScaleAxis::XNeg    => Vec3::NEG_X * size_diff.x * 0.5,
                ScaleAxis::YPos    => Vec3::Y     * size_diff.y * 0.5,
                ScaleAxis::YNeg    => Vec3::NEG_Y * size_diff.y * 0.5,
                ScaleAxis::ZPos    => Vec3::Z     * size_diff.z * 0.5,
                ScaleAxis::ZNeg    => Vec3::NEG_Z * size_diff.z * 0.5,
                ScaleAxis::Uniform => Vec3::ZERO,
            };
            let world_offset = state.drag_rotation * gizmo_offset;
            let new_pos = initial_pos + world_offset;

            // Record undo BEFORE we mutate.
            let size_changed = (initial_size - new_size).length() > 0.001;
            let pos_changed  = (initial_pos - new_pos).length() > 0.001;
            if size_changed || pos_changed {
                old_states.push((entity.to_bits(), initial_pos.to_array(), initial_size.to_array()));
                new_states.push((entity.to_bits(), new_pos.to_array(),     new_size.to_array()));
            }

            // Apply — file-system-first parts are unit-scale GLBs so
            // size lands on Transform.scale. BasePart.size + cframe
            // stay the authoritative source the TOML writer reads.
            // Sanitize: any NaN/inf reaching Transform.translation
            // panics Avian's Position validator on the next physics step.
            let new_pos = sane_translation(new_pos);
            let new_size = sane_size(new_size);
            transform.translation = new_pos;
            transform.scale = new_size;
            if let Some(mut bp) = basepart_opt {
                bp.size = new_size;
                bp.cframe.translation = new_pos;
            }
        }

        if !old_states.is_empty() {
            undo_stack.push(crate::undo::Action::ScaleEntities { old_states, new_states });
        }

        state.dragged_axis = None;
        state.dragged_entity = None;
        state.initial_scales.clear();
        state.initial_positions.clear();
    }
}

// ============================================================================
// 5. Public Helpers
// ============================================================================

/// Check if a ray hits any scale handle for the GROUP bounds.
/// Layout matches `scale_handles::sync_scale_handle_root` — face cubes
/// at `face_extent + handle_ext` along each axis (rotated by `rotation`),
/// plus the uniform-scale cube at the group center.
///
/// - `group_extent` — per-axis half-size of the group AABB. Used for
///   per-axis face anchoring (a tall thin part puts Y handles further
///   out than X).
/// - `screen_scale` — camera-distance gizmo scale from
///   [`compute_scale_screen_scale`]. Cube and shaft sizes derive from
///   this so handles stay constant on screen.
/// - `rotation` — `Quat::IDENTITY` for World mode, active entity's
///   rotation for Local mode. Must match the visual layout's rotation.
pub fn is_clicking_scale_handle_group(
    ray: &Ray3d,
    group_center: Vec3,
    group_extent: Vec3,
    screen_scale: f32,
    rotation: Quat,
) -> bool {
    use crate::scale_handles::{SCREEN_HANDLE_EXT, SCREEN_CUBE_SIZE, SCREEN_CENTER_SIZE};

    let handle_ext = screen_scale * SCREEN_HANDLE_EXT;
    let cube_size = screen_scale * SCREEN_CUBE_SIZE;
    let center_size = screen_scale * SCREEN_CENTER_SIZE;
    let hit_radius = (cube_size * 0.75).max(0.05);
    let center_hit_radius = (center_size * 0.75).max(0.05);

    let dirs: [(Vec3, f32); 6] = [
        (Vec3::X,     group_extent.x),
        (Vec3::NEG_X, group_extent.x),
        (Vec3::Y,     group_extent.y),
        (Vec3::NEG_Y, group_extent.y),
        (Vec3::Z,     group_extent.z),
        (Vec3::NEG_Z, group_extent.z),
    ];

    // Face cubes — anchor at per-axis face + constant screen offset.
    for (dir, face) in dirs {
        let world_pos = group_center + rotation * dir * (face + handle_ext);
        if ray_to_point_distance(ray.origin, *ray.direction, world_pos) < hit_radius {
            return true;
        }
    }
    // Uniform center cube
    if ray_to_point_distance(ray.origin, *ray.direction, group_center) < center_hit_radius {
        return true;
    }
    false
}

/// Convenience: compute the camera-distance screen scale for the scale
/// gizmo, matching `scale_handles::sync_scale_handle_root`. Pass the
/// result into [`is_clicking_scale_handle_group`] so visual + hit zones
/// stay in lockstep.
pub fn compute_scale_screen_scale(
    group_center: Vec3,
    camera_translation: Vec3,
    fov: f32,
) -> f32 {
    use crate::scale_handles::SCREEN_FRACTION;
    let cam_dist = (group_center - camera_translation).length().max(0.1);
    cam_dist * (fov * 0.5).tan() * SCREEN_FRACTION
}

// ============================================================================
// Private Helpers
// ============================================================================

fn compute_new_size(axis: ScaleAxis, initial: Vec3, drag: f32) -> Vec3 {
    // Sanitize inputs — a degenerate part (size component = 0, NaN, or inf)
    // would propagate non-finite values through the drag math and end up in
    // Transform.translation, which Avian then rejects with a panic. Floor
    // every component to 0.1 before any arithmetic.
    let initial = Vec3::new(
        sane_pos(initial.x).max(0.1),
        sane_pos(initial.y).max(0.1),
        sane_pos(initial.z).max(0.1),
    );
    let drag = if drag.is_finite() { drag } else { 0.0 };

    let raw = match axis {
        ScaleAxis::XPos | ScaleAxis::XNeg => Vec3::new((initial.x + drag).max(0.1), initial.y, initial.z),
        ScaleAxis::YPos | ScaleAxis::YNeg => Vec3::new(initial.x, (initial.y + drag).max(0.1), initial.z),
        ScaleAxis::ZPos | ScaleAxis::ZNeg => Vec3::new(initial.x, initial.y, (initial.z + drag).max(0.1)),
        ScaleAxis::Uniform => {
            // `initial.max_element()` is now guaranteed >= 0.1 by the
            // sanitize step above, so `drag / m` cannot divide by zero.
            let m = initial.max_element();
            let f = (1.0 + drag / m).max(0.1);
            initial * f
        }
    };
    sane_size(raw)
}

#[inline]
fn sane_size(v: Vec3) -> Vec3 {
    Vec3::new(
        if v.x.is_finite() { v.x.max(0.1) } else { 0.1 },
        if v.y.is_finite() { v.y.max(0.1) } else { 0.1 },
        if v.z.is_finite() { v.z.max(0.1) } else { 0.1 },
    )
}

/// Replace a non-finite scalar with 0.0. Used inside `compute_new_size`
/// before the floor-to-0.1 step so NaN initial sizes don't leak into
/// drag arithmetic. Position-clamping for `Transform.translation`
/// lives in `instance_loader::safe_translation`; this is purely a
/// per-axis NaN guard for size-space math.
#[inline]
fn sane_pos(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// NaN-and-distance-clamping translation guard. Routes through the
/// centralized `instance_loader::safe_translation` helper so the
/// scale-tool inherits the same `MAX_WORLD_EXTENT` (5000) cap that
/// move + select use. Falls back to origin if no better fallback is
/// available; callers that have an `initial_pos` should pass it
/// directly to `safe_translation` for a less jarring recovery.
#[inline]
fn sane_translation(v: Vec3) -> Vec3 {
    crate::space::instance_loader::safe_translation(v, Vec3::ZERO)
}

fn apply_snap(size: Vec3, settings: &crate::editor_settings::EditorSettings) -> Vec3 {
    const MIN: f32 = 0.1;
    if settings.snap_enabled {
        // Guard against snap_size = 0 (would produce inf in `size / s`).
        let s = settings.snap_size.max(0.001);
        Vec3::new(
            ((size.x / s).round() * s).max(MIN),
            ((size.y / s).round() * s).max(MIN),
            ((size.z / s).round() * s).max(MIN),
        )
    } else {
        size.max(Vec3::splat(MIN))
    }
}

fn apply_size_to_entity(
    transform: &mut Transform,
    basepart_opt: Option<Mut<crate::classes::BasePart>>,
    part_opt: Option<&crate::classes::Part>,
    mesh_opt: Option<Mut<Mesh3d>>,
    meshes: &mut Assets<Mesh>,
    size: Vec3,
    pos: Vec3,
    has_mesh_source: bool,
    // When `false`, skip the per-frame primitive mesh regeneration and
    // express size via Transform.scale instead. Per-frame mesh regen
    // during drag (1 mesh asset per frame, GPU upload, then dropped)
    // is the dominant source of scale-tool input lag; deferring the
    // regen to drag-release is functionally identical visually.
    regenerate_mesh: bool,
    // Size already baked into the cached mesh — needed only on the
    // legacy "no mesh source + skip regen" path so we can compute
    // `transform.scale = size / mesh_baked_size` and have the visual
    // dimensions match `size`. Pass `Vec3::ONE` if irrelevant.
    mesh_baked_size: Vec3,
) {
    // Defense-in-depth sanitize — callers should already pass clean values,
    // but a single NaN reaching Transform.translation panics Avian
    // (`avian3d/src/schedule/mod.rs:313: NaN or infinity found in Avian
    // component: type=Position`). Drop the bad component to a safe default
    // rather than letting it propagate.
    let size = sane_size(size);
    let pos = sane_translation(pos);

    transform.translation = pos;

    if let Some(mut bp) = basepart_opt {
        bp.size = size;
        bp.cframe.translation = pos;
    }

    if has_mesh_source {
        // File-system-first: .glb mesh is unit-scale, apply size via Transform.scale.
        transform.scale = size;
    } else if regenerate_mesh {
        // Legacy + final commit: bake `size` into the mesh, scale = ONE.
        transform.scale = Vec3::ONE;
        if let (Some(part), Some(mut mesh3d)) = (part_opt, mesh_opt) {
            let new_mesh = match part.shape {
                crate::classes::PartType::Block    => meshes.add(bevy::math::primitives::Cuboid::from_size(size)),
                crate::classes::PartType::Ball     => meshes.add(bevy::math::primitives::Sphere::new(size.x / 2.0)),
                crate::classes::PartType::Cylinder => meshes.add(bevy::math::primitives::Cylinder::new(size.x / 2.0, size.y)),
                _                                  => meshes.add(bevy::math::primitives::Cuboid::from_size(size)),
            };
            mesh3d.0 = new_mesh;
        }
    } else {
        // Legacy + mid-drag: defer regen. Cached mesh is at `mesh_baked_size`;
        // visually scale to `size` via Transform.scale. On drag release the
        // caller invokes `apply_size_to_entity(..., true)` once to bake
        // `size` into a fresh mesh and restore scale = ONE.
        let bake = sane_size(mesh_baked_size);
        transform.scale = Vec3::new(
            size.x / bake.x,
            size.y / bake.y,
            size.z / bake.z,
        );
    }
}

fn is_descendant(
    entity: Entity,
    selected_set: &std::collections::HashSet<Entity>,
    parent_query: &Query<&ChildOf>,
) -> bool {
    let mut current = entity;
    while let Ok(child_of) = parent_query.get(current) {
        let parent = child_of.parent();
        if selected_set.contains(&parent) { return true; }
        current = parent;
    }
    false
}

/// Hit-test the scale gizmo handles against a ray and return the picked
/// axis along with the group's center, extent, and gizmo rotation. Used
/// for both hover detection (every frame) and click-to-drag.
///
/// `selected_iter` yields one `(translation, rotation, size)` tuple per
/// selected entity — the function rebuilds the same group AABB and
/// rotation as `scale_handles::sync_scale_handle_root` so visual cube
/// position and hit zone stay perfectly aligned.
fn pick_scale_handle(
    ray_origin: Vec3,
    ray_direction: Vec3,
    cam_position: Vec3,
    fov: f32,
    transform_mode: crate::ui::TransformMode,
    selected: &[(Vec3, Quat, Vec3)],
) -> Option<(ScaleAxis, Vec3, Vec3, Quat)> {
    use crate::scale_handles::{
        SCREEN_FRACTION, SCREEN_HANDLE_EXT, SCREEN_CUBE_SIZE, SCREEN_CENTER_SIZE,
    };

    if selected.is_empty() { return None; }

    let mut bounds_min = Vec3::splat(f32::MAX);
    let mut bounds_max = Vec3::splat(f32::MIN);
    for &(pos, rot, size) in selected {
        let (mn, mx) = crate::math_utils::calculate_rotated_aabb(pos, size * 0.5, rot);
        bounds_min = bounds_min.min(mn);
        bounds_max = bounds_max.max(mx);
    }

    let group_center = (bounds_min + bounds_max) * 0.5;
    let group_extent = (bounds_max - bounds_min) * 0.5;

    let cam_dist = (group_center - cam_position).length().max(0.1);
    let screen_scale = cam_dist * (fov * 0.5).tan() * SCREEN_FRACTION;

    let handle_ext = screen_scale * SCREEN_HANDLE_EXT;
    let cube_size = screen_scale * SCREEN_CUBE_SIZE;
    let center_size = screen_scale * SCREEN_CENTER_SIZE;
    let hit_radius = (cube_size * 0.75).max(0.05);
    let center_hit_radius = (center_size * 0.75).max(0.05);

    let rotation = crate::move_tool::gizmo_rotation_for(
        transform_mode,
        selected.iter().map(|&(_, rot, _)| rot),
    );

    let dirs: &[(ScaleAxis, Vec3, f32)] = &[
        (ScaleAxis::XPos, Vec3::X,     group_extent.x),
        (ScaleAxis::XNeg, Vec3::NEG_X, group_extent.x),
        (ScaleAxis::YPos, Vec3::Y,     group_extent.y),
        (ScaleAxis::YNeg, Vec3::NEG_Y, group_extent.y),
        (ScaleAxis::ZPos, Vec3::Z,     group_extent.z),
        (ScaleAxis::ZNeg, Vec3::NEG_Z, group_extent.z),
    ];

    let mut best: Option<(ScaleAxis, f32)> = None;
    for &(ax, dir, face) in dirs {
        let world_pos = group_center + rotation * dir * (face + handle_ext);
        let d = ray_to_point_distance(ray_origin, ray_direction, world_pos);
        if d < hit_radius {
            if best.map_or(true, |(_, dist)| d < dist) {
                best = Some((ax, d));
            }
        }
    }
    // Center cube — uniform scale handle. Tested last so it doesn't
    // out-prioritize a face cube if both happen to fall under the
    // cursor (face cubes are visually further out, so prefer them).
    let d = ray_to_point_distance(ray_origin, ray_direction, group_center);
    if d < center_hit_radius {
        if best.map_or(true, |(_, dist)| d < dist) {
            best = Some((ScaleAxis::Uniform, d));
        }
    }

    best.map(|(axis, _)| (axis, group_center, group_extent, rotation))
}
